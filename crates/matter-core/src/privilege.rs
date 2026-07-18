//! Privilege claims, withhold holds, protocol stub, and privilege-log CSV export
//! (schema v12 / track 0031).
//!
//! ## Production contract for **0040**
//!
//! - [`Matter::item_is_withheld`] / [`Matter::list_withheld_item_ids`] are the
//!   gate APIs: production **must** skip or fail-closed on withheld items.
//! - Soft-clear retains `item_privilege.description` for internal audit.
//!   Production load-file / natives metadata **must not** emit
//!   `item_privilege.description` (or basis narrative) for `status=cleared`
//!   rows, and should default-exclude privilege description fields entirely.
//! - Privilege log export **never** includes `cleared` rows.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Write;

use camino::Utf8PathBuf;
use rusqlite::{params, params_from_iter, types::Value, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::audit::{self, AuditEventInput};
use crate::error::{Error, Result};
use crate::filter::{SCOPE_ENTIRE_MATTER, SCOPE_REVIEW_CORPUS};
use crate::matter::{normalize_actor, now_rfc3339, Matter};

// ---------------------------------------------------------------------------
// Vocabulary
// ---------------------------------------------------------------------------

/// Privilege basis (type) vocabulary (P0 single enum per item).
pub mod privilege_basis {
    pub const ATTORNEY_CLIENT: &str = "attorney_client";
    pub const WORK_PRODUCT: &str = "work_product";
    pub const ATTORNEY_CLIENT_WORK_PRODUCT: &str = "attorney_client_work_product";
    pub const COMMON_INTEREST: &str = "common_interest";
    pub const OTHER: &str = "other";

    pub const ALL: &[&str] = &[
        ATTORNEY_CLIENT,
        WORK_PRODUCT,
        ATTORNEY_CLIENT_WORK_PRODUCT,
        COMMON_INTEREST,
        OTHER,
    ];
}

/// Privilege claim status vocabulary.
pub mod privilege_status {
    pub const ASSERTED: &str = "asserted";
    pub const UNDER_REVIEW: &str = "under_review";
    pub const CLEARED: &str = "cleared";
    pub const PARTIAL_REDACTION: &str = "partial_redaction";

    pub const ALL: &[&str] = &[ASSERTED, UNDER_REVIEW, CLEARED, PARTIAL_REDACTION];

    /// Statuses that count as an active privilege claim (not cleared).
    pub const ACTIVE: &[&str] = &[ASSERTED, UNDER_REVIEW, PARTIAL_REDACTION];
}

/// Matter protocol log-format vocabulary.
pub mod privilege_log_format {
    pub const STANDARD: &str = "standard";
    pub const AUTOMATED_METADATA: &str = "automated_metadata";
    pub const CATEGORY: &str = "category";

    pub const ALL: &[&str] = &[STANDARD, AUTOMATED_METADATA, CATEGORY];
}

/// Human label for a basis key (UI / CSV PrivilegeType column).
pub fn basis_label(basis: &str) -> &'static str {
    match basis {
        privilege_basis::ATTORNEY_CLIENT => "Attorney-Client Privilege",
        privilege_basis::WORK_PRODUCT => "Work Product",
        privilege_basis::ATTORNEY_CLIENT_WORK_PRODUCT => "Attorney-Client and Work Product",
        privilege_basis::COMMON_INTEREST => "Common Interest",
        privilege_basis::OTHER => "Other (see description)",
        _ => "Other (see description)",
    }
}

/// Privilege log CSV header columns (exact order, §3.4.2).
pub const PRIVILEGE_LOG_COLUMNS: &[&str] = &[
    "ControlNumber",
    "ParentControlNumber",
    "FamilyId",
    "Custodian",
    "DocDate",
    "From",
    "To",
    "Cc",
    "Bcc",
    "Subject",
    "FileName",
    "FileType",
    "PrivilegeType",
    "Description",
    "Status",
    "Withhold",
    "HasPrivilegeCode",
    "MatterId",
    "ExportedAt",
];

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// 1:1 privilege claim on an item (schema v12).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemPrivilege {
    pub item_id: String,
    pub matter_id: String,
    pub basis: String,
    pub description: String,
    pub status: String,
    /// 0/1 production hold.
    pub withhold: i64,
    /// 0/1 include on privilege log export.
    pub include_on_log: i64,
    pub asserted_at: Option<String>,
    pub asserted_by: Option<String>,
    pub updated_at: String,
    pub updated_by: String,
    pub extra_json: Option<String>,
}

/// Input for [`Matter::upsert_item_privilege`].
#[derive(Debug, Clone)]
pub struct UpsertItemPrivilegeInput {
    pub item_id: String,
    pub basis: String,
    pub description: String,
    pub status: String,
    pub withhold: bool,
    pub include_on_log: bool,
    pub actor: String,
}

/// Matter-level privilege protocol stub (502(d)/502(e) notes are informational).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrivilegeProtocol {
    pub matter_id: String,
    pub log_format: String,
    pub fre_502d_note: Option<String>,
    pub fre_502e_note: Option<String>,
    /// 1 = warn on blank description for include_on_log rows (export still proceeds).
    pub description_required: i64,
    pub updated_at: String,
    pub updated_by: String,
}

/// Input for [`Matter::upsert_privilege_protocol`].
#[derive(Debug, Clone)]
pub struct UpsertPrivilegeProtocolInput {
    pub log_format: String,
    pub fre_502d_note: Option<String>,
    pub fre_502e_note: Option<String>,
    pub description_required: bool,
    pub actor: String,
}

/// Result of [`Matter::family_privilege_consistency`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FamilyPrivilegeConsistency {
    pub consistent: bool,
    pub privileged_ids: Vec<String>,
    pub non_privileged_ids: Vec<String>,
}

/// Scope / path for [`Matter::export_privilege_log`].
#[derive(Debug, Clone)]
pub struct PrivilegeLogExportParams {
    /// `review_corpus` or `entire_matter` (see filter scope constants).
    pub scope: String,
    /// Destination CSV path (parent dirs created if needed).
    pub path: Utf8PathBuf,
    /// Optional explicit item id filter (intersected with eligibility + scope).
    pub filter_ids: Option<Vec<String>>,
}

/// Result of a privilege log export.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrivilegeLogExportResult {
    pub path: String,
    pub row_count: u64,
    pub blank_description_count: u64,
    pub withheld_count: u64,
}

// ---------------------------------------------------------------------------
// Conn-level helpers (used inside apply_codes txn + public Matter methods)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PrivilegeEnsureChange {
    /// No row change (already asserted / under_review / partial).
    Unchanged,
    /// Inserted or re-asserted from cleared.
    Changed,
}

/// Ensure an asserted privilege row exists for `item_id` (coding apply path).
///
/// Defaults: status=asserted, withhold=1, include_on_log=1, basis=attorney_client.
/// Soft-cleared rows are re-asserted (description retained). Already-active rows
/// are left unchanged.
pub(crate) fn ensure_item_privilege_conn(
    conn: &Connection,
    matter_id: &str,
    item_id: &str,
    actor: &str,
    now: &str,
) -> Result<PrivilegeEnsureChange> {
    let existing: Option<(String, i64, i64, String)> = conn
        .query_row(
            "SELECT status, withhold, include_on_log, basis FROM item_privilege \
             WHERE item_id = ?1 AND matter_id = ?2",
            params![item_id, matter_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()?;

    match existing {
        Some((status, withhold, include_on_log, _basis)) => {
            if privilege_status::ACTIVE.contains(&status.as_str())
                && withhold == 1
                && include_on_log == 1
            {
                // Ensure denorm cache is correct even if somehow drifted.
                set_item_withhold_cache(conn, matter_id, item_id, true)?;
                return Ok(PrivilegeEnsureChange::Unchanged);
            }
            // Re-assert from cleared or fix withhold/include flags.
            conn.execute(
                "UPDATE item_privilege SET \
                    status = ?1, withhold = 1, include_on_log = 1, \
                    updated_at = ?2, updated_by = ?3 \
                 WHERE item_id = ?4 AND matter_id = ?5",
                params![privilege_status::ASSERTED, now, actor, item_id, matter_id],
            )?;
            set_item_withhold_cache(conn, matter_id, item_id, true)?;
            Ok(PrivilegeEnsureChange::Changed)
        }
        None => {
            conn.execute(
                "INSERT INTO item_privilege (\
                    item_id, matter_id, basis, description, status, withhold, \
                    include_on_log, asserted_at, asserted_by, updated_at, updated_by, extra_json\
                 ) VALUES (?1, ?2, ?3, '', ?4, 1, 1, ?5, ?6, ?5, ?6, NULL)",
                params![
                    item_id,
                    matter_id,
                    privilege_basis::ATTORNEY_CLIENT,
                    privilege_status::ASSERTED,
                    now,
                    actor,
                ],
            )?;
            set_item_withhold_cache(conn, matter_id, item_id, true)?;
            Ok(PrivilegeEnsureChange::Changed)
        }
    }
}

/// Soft-clear privilege claim (coding remove path): status=cleared, withhold=0,
/// include_on_log=0; retain description/basis.
pub(crate) fn soft_clear_item_privilege_conn(
    conn: &Connection,
    matter_id: &str,
    item_id: &str,
    actor: &str,
    now: &str,
) -> Result<bool> {
    let existing: Option<String> = conn
        .query_row(
            "SELECT status FROM item_privilege WHERE item_id = ?1 AND matter_id = ?2",
            params![item_id, matter_id],
            |row| row.get(0),
        )
        .optional()?;
    let Some(status) = existing else {
        set_item_withhold_cache(conn, matter_id, item_id, false)?;
        return Ok(false);
    };
    if status == privilege_status::CLEARED {
        set_item_withhold_cache(conn, matter_id, item_id, false)?;
        return Ok(false);
    }
    conn.execute(
        "UPDATE item_privilege SET \
            status = ?1, withhold = 0, include_on_log = 0, \
            updated_at = ?2, updated_by = ?3 \
         WHERE item_id = ?4 AND matter_id = ?5",
        params![privilege_status::CLEARED, now, actor, item_id, matter_id],
    )?;
    set_item_withhold_cache(conn, matter_id, item_id, false)?;
    Ok(true)
}

fn set_item_withhold_cache(
    conn: &Connection,
    matter_id: &str,
    item_id: &str,
    withhold: bool,
) -> Result<()> {
    conn.execute(
        "UPDATE items SET privilege_withhold = ?1 WHERE id = ?2 AND matter_id = ?3",
        params![if withhold { 1i64 } else { 0i64 }, item_id, matter_id],
    )?;
    Ok(())
}

fn map_privilege_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ItemPrivilege> {
    Ok(ItemPrivilege {
        item_id: row.get(0)?,
        matter_id: row.get(1)?,
        basis: row.get(2)?,
        description: row.get(3)?,
        status: row.get(4)?,
        withhold: row.get(5)?,
        include_on_log: row.get(6)?,
        asserted_at: row.get(7)?,
        asserted_by: row.get(8)?,
        updated_at: row.get(9)?,
        updated_by: row.get(10)?,
        extra_json: row.get(11)?,
    })
}

const PRIVILEGE_SELECT: &str = "item_id, matter_id, basis, description, status, withhold, \
    include_on_log, asserted_at, asserted_by, updated_at, updated_by, extra_json";

fn validate_basis(basis: &str) -> Result<()> {
    if privilege_basis::ALL.contains(&basis) {
        Ok(())
    } else {
        Err(Error::Other(format!(
            "invalid privilege basis '{basis}'; expected one of: {}",
            privilege_basis::ALL.join(", ")
        )))
    }
}

fn validate_status(status: &str) -> Result<()> {
    if privilege_status::ALL.contains(&status) {
        Ok(())
    } else {
        Err(Error::Other(format!(
            "invalid privilege status '{status}'; expected one of: {}",
            privilege_status::ALL.join(", ")
        )))
    }
}

fn validate_log_format(fmt: &str) -> Result<()> {
    if privilege_log_format::ALL.contains(&fmt) {
        Ok(())
    } else {
        Err(Error::Other(format!(
            "invalid privilege log_format '{fmt}'; expected one of: {}",
            privilege_log_format::ALL.join(", ")
        )))
    }
}

/// Whether an item counts as "privileged" for family consistency.
///
/// Active privilege status **or** presence of the seed `privilege` code.
fn item_is_privileged_for_family(
    conn: &Connection,
    matter_id: &str,
    item_id: &str,
) -> Result<bool> {
    let status: Option<String> = conn
        .query_row(
            "SELECT status FROM item_privilege WHERE item_id = ?1 AND matter_id = ?2",
            params![item_id, matter_id],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(ref s) = status {
        if privilege_status::ACTIVE.contains(&s.as_str()) {
            return Ok(true);
        }
    }
    let has_code: bool = conn.query_row(
        "SELECT COUNT(*) > 0 FROM item_codes ic \
         JOIN code_definitions cd ON cd.id = ic.code_id \
         WHERE ic.item_id = ?1 AND cd.matter_id = ?2 AND cd.key = 'privilege'",
        params![item_id, matter_id],
        |row| row.get(0),
    )?;
    Ok(has_code)
}

// ---------------------------------------------------------------------------
// CSV helpers
// ---------------------------------------------------------------------------

/// RFC4180 field escape (always quote when needed).
pub fn csv_escape_field(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') || s.contains('\r') {
        let mut out = String::with_capacity(s.len() + 2);
        out.push('"');
        for ch in s.chars() {
            if ch == '"' {
                out.push('"');
                out.push('"');
            } else {
                out.push(ch);
            }
        }
        out.push('"');
        out
    } else {
        s.to_string()
    }
}

/// Join JSON string-array addresses for log display.
pub fn join_addrs_json(raw: Option<&str>) -> String {
    let Some(raw) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return String::new();
    };
    match serde_json::from_str::<Vec<String>>(raw) {
        Ok(addrs) => addrs
            .into_iter()
            .map(|a| a.trim().to_string())
            .filter(|a| !a.is_empty())
            .collect::<Vec<_>>()
            .join("; "),
        Err(_) => raw.to_string(),
    }
}

/// Basename of a matter path (or empty).
pub fn path_basename(path: Option<&str>) -> String {
    let Some(p) = path.map(str::trim).filter(|s| !s.is_empty()) else {
        return String::new();
    };
    // Accept both separators.
    p.rsplit(['/', '\\']).next().unwrap_or(p).to_string()
}

fn first_nonempty(a: Option<&str>, b: Option<&str>) -> String {
    a.map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .or_else(|| {
            b.map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
        })
        .unwrap_or_default()
}

fn doc_date_from(sent: Option<&str>, received: Option<&str>, created: Option<&str>) -> String {
    // Prefer sent → received → created (item then parent handled by caller).
    if let Some(s) = sent.map(str::trim).filter(|s| !s.is_empty()) {
        return s.to_string();
    }
    if let Some(s) = received.map(str::trim).filter(|s| !s.is_empty()) {
        return s.to_string();
    }
    if let Some(s) = created.map(str::trim).filter(|s| !s.is_empty()) {
        return s.to_string();
    }
    String::new()
}

fn yn(flag: bool) -> &'static str {
    if flag {
        "Y"
    } else {
        "N"
    }
}

// ---------------------------------------------------------------------------
// Matter API
// ---------------------------------------------------------------------------

impl Matter {
    /// Ensure privilege claim for an item (asserted, withhold=1, include_on_log=1).
    ///
    /// Idempotent for already-active rows. Audits `privilege.upsert` when state changes.
    pub fn ensure_item_privilege(&self, item_id: &str, actor: &str) -> Result<ItemPrivilege> {
        self.ensure_item_in_matter(item_id)?;
        let actor = normalize_actor(actor);
        let now = now_rfc3339();
        let mut changed = PrivilegeEnsureChange::Unchanged;
        self.with_transaction(|conn| {
            changed = ensure_item_privilege_conn(conn, self.id(), item_id, &actor, &now)?;
            if changed == PrivilegeEnsureChange::Changed {
                let row = load_privilege(conn, self.id(), item_id)?
                    .ok_or_else(|| Error::Other("privilege row missing after ensure".into()))?;
                let params_json = serde_json::json!({
                    "item_ids": [item_id],
                    "op": "ensure",
                    "basis": row.basis,
                    "status": row.status,
                    "withhold": row.withhold,
                    "include_on_log": row.include_on_log,
                    "description": row.description,
                })
                .to_string();
                audit::append_event(
                    conn,
                    &AuditEventInput {
                        actor: actor.clone(),
                        action: "privilege.upsert".into(),
                        entity: format!("item:{item_id}"),
                        params_json,
                        tool_version: env!("CARGO_PKG_VERSION").into(),
                    },
                    &now,
                )?;
            }
            Ok(())
        })?;
        self.get_item_privilege(item_id)?
            .ok_or_else(|| Error::Other(format!("privilege not found after ensure for {item_id}")))
    }

    /// Create or update privilege claim fields. Validates basis/status enums.
    pub fn upsert_item_privilege(&self, input: UpsertItemPrivilegeInput) -> Result<ItemPrivilege> {
        self.ensure_item_in_matter(&input.item_id)?;
        let basis = input.basis.trim();
        let status = input.status.trim();
        validate_basis(basis)?;
        validate_status(status)?;
        let actor = normalize_actor(&input.actor);
        let now = now_rfc3339();
        let description = input.description; // allow blank
        let withhold = if input.withhold { 1i64 } else { 0i64 };
        let include_on_log = if input.include_on_log { 1i64 } else { 0i64 };
        // Cleared implies withhold=0 / include_on_log=0 for consistency when
        // status is set to cleared via panel (soft-clear semantics).
        let (withhold, include_on_log) = if status == privilege_status::CLEARED {
            (0i64, 0i64)
        } else {
            (withhold, include_on_log)
        };
        let item_id = input.item_id.clone();

        self.with_transaction(|conn| {
            let existing = load_privilege(conn, self.id(), &item_id)?;
            let (asserted_at, asserted_by) = match &existing {
                Some(e) if e.asserted_at.is_some() => {
                    (e.asserted_at.clone(), e.asserted_by.clone())
                }
                _ if status != privilege_status::CLEARED => {
                    (Some(now.clone()), Some(actor.clone()))
                }
                Some(e) => (e.asserted_at.clone(), e.asserted_by.clone()),
                None => (None, None),
            };

            if existing.is_some() {
                conn.execute(
                    "UPDATE item_privilege SET \
                        basis = ?1, description = ?2, status = ?3, withhold = ?4, \
                        include_on_log = ?5, asserted_at = ?6, asserted_by = ?7, \
                        updated_at = ?8, updated_by = ?9 \
                     WHERE item_id = ?10 AND matter_id = ?11",
                    params![
                        basis,
                        description,
                        status,
                        withhold,
                        include_on_log,
                        asserted_at,
                        asserted_by,
                        now,
                        actor,
                        item_id,
                        self.id(),
                    ],
                )?;
            } else {
                conn.execute(
                    "INSERT INTO item_privilege (\
                        item_id, matter_id, basis, description, status, withhold, \
                        include_on_log, asserted_at, asserted_by, updated_at, updated_by, extra_json\
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, NULL)",
                    params![
                        item_id,
                        self.id(),
                        basis,
                        description,
                        status,
                        withhold,
                        include_on_log,
                        asserted_at,
                        asserted_by,
                        now,
                        actor,
                    ],
                )?;
            }
            set_item_withhold_cache(conn, self.id(), &item_id, withhold == 1)?;

            let params_json = serde_json::json!({
                "item_ids": [&item_id],
                "op": "upsert",
                "basis": basis,
                "status": status,
                "withhold": withhold,
                "include_on_log": include_on_log,
                "description": description,
            })
            .to_string();
            audit::append_event(
                conn,
                &AuditEventInput {
                    actor: actor.clone(),
                    action: "privilege.upsert".into(),
                    entity: format!("item:{item_id}"),
                    params_json,
                    tool_version: env!("CARGO_PKG_VERSION").into(),
                },
                &now,
            )?;
            Ok(())
        })?;

        self.get_item_privilege(&item_id)?
            .ok_or_else(|| Error::Other(format!("privilege not found after upsert for {item_id}")))
    }

    /// Soft-clear privilege claim: status=cleared, withhold=0, include_on_log=0.
    /// Description is retained for internal audit / re-open.
    pub fn clear_item_privilege(&self, item_id: &str, actor: &str) -> Result<()> {
        self.ensure_item_in_matter(item_id)?;
        let actor = normalize_actor(actor);
        let now = now_rfc3339();
        self.with_transaction(|conn| {
            let existing = load_privilege(conn, self.id(), item_id)?;
            let changed = soft_clear_item_privilege_conn(conn, self.id(), item_id, &actor, &now)?;
            if changed {
                let description = existing
                    .as_ref()
                    .map(|e| e.description.clone())
                    .unwrap_or_default();
                let params_json = serde_json::json!({
                    "item_ids": [item_id],
                    "op": "clear",
                    "description_retained": true,
                    "description": description,
                })
                .to_string();
                audit::append_event(
                    conn,
                    &AuditEventInput {
                        actor: actor.clone(),
                        action: "privilege.clear".into(),
                        entity: format!("item:{item_id}"),
                        params_json,
                        tool_version: env!("CARGO_PKG_VERSION").into(),
                    },
                    &now,
                )?;
            }
            Ok(())
        })
    }

    /// Load privilege claim for an item, if any.
    pub fn get_item_privilege(&self, item_id: &str) -> Result<Option<ItemPrivilege>> {
        self.ensure_item_in_matter(item_id)?;
        load_privilege(self.connection(), self.id(), item_id)
    }

    /// Load privilege claims for many items (missing ids omitted).
    pub fn list_item_privilege(
        &self,
        item_ids: &[String],
    ) -> Result<HashMap<String, ItemPrivilege>> {
        if item_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let mut out = HashMap::new();
        // Chunk to keep SQL size reasonable.
        for chunk in item_ids.chunks(200) {
            let placeholders = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
            let sql = format!(
                "SELECT {PRIVILEGE_SELECT} FROM item_privilege \
                 WHERE matter_id = ? AND item_id IN ({placeholders})"
            );
            let mut params: Vec<Value> = Vec::with_capacity(1 + chunk.len());
            params.push(Value::Text(self.id().to_string()));
            for id in chunk {
                params.push(Value::Text(id.clone()));
            }
            let mut stmt = self.connection().prepare(&sql)?;
            let rows = stmt.query_map(params_from_iter(params), map_privilege_row)?;
            for row in rows {
                let p = row?;
                out.insert(p.item_id.clone(), p);
            }
        }
        Ok(out)
    }

    /// Get matter privilege protocol (defaults if never upserted).
    pub fn get_privilege_protocol(&self) -> Result<PrivilegeProtocol> {
        let row: Option<PrivilegeProtocol> = self
            .connection()
            .query_row(
                "SELECT matter_id, log_format, fre_502d_note, fre_502e_note, \
                        description_required, updated_at, updated_by \
                 FROM privilege_protocol WHERE matter_id = ?1",
                params![self.id()],
                |row| {
                    Ok(PrivilegeProtocol {
                        matter_id: row.get(0)?,
                        log_format: row.get(1)?,
                        fre_502d_note: row.get(2)?,
                        fre_502e_note: row.get(3)?,
                        description_required: row.get(4)?,
                        updated_at: row.get(5)?,
                        updated_by: row.get(6)?,
                    })
                },
            )
            .optional()?;
        Ok(row.unwrap_or_else(|| PrivilegeProtocol {
            matter_id: self.id().to_string(),
            log_format: privilege_log_format::STANDARD.to_string(),
            fre_502d_note: None,
            fre_502e_note: None,
            description_required: 1,
            updated_at: String::new(),
            updated_by: String::new(),
        }))
    }

    /// Upsert matter privilege protocol stub. Audits `privilege.protocol_upsert`.
    pub fn upsert_privilege_protocol(
        &self,
        input: UpsertPrivilegeProtocolInput,
    ) -> Result<PrivilegeProtocol> {
        let fmt = input.log_format.trim();
        validate_log_format(fmt)?;
        let actor = normalize_actor(&input.actor);
        let now = now_rfc3339();
        let description_required = if input.description_required {
            1i64
        } else {
            0i64
        };
        let fre_502d = input
            .fre_502d_note
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());
        let fre_502e = input
            .fre_502e_note
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());

        self.with_transaction(|conn| {
            conn.execute(
                "INSERT INTO privilege_protocol (\
                    matter_id, log_format, fre_502d_note, fre_502e_note, \
                    description_required, updated_at, updated_by\
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) \
                 ON CONFLICT(matter_id) DO UPDATE SET \
                    log_format = excluded.log_format, \
                    fre_502d_note = excluded.fre_502d_note, \
                    fre_502e_note = excluded.fre_502e_note, \
                    description_required = excluded.description_required, \
                    updated_at = excluded.updated_at, \
                    updated_by = excluded.updated_by",
                params![
                    self.id(),
                    fmt,
                    fre_502d,
                    fre_502e,
                    description_required,
                    now,
                    actor,
                ],
            )?;
            let params_json = serde_json::json!({
                "log_format": fmt,
                "fre_502d_note": fre_502d,
                "fre_502e_note": fre_502e,
                "description_required": description_required,
            })
            .to_string();
            audit::append_event(
                conn,
                &AuditEventInput {
                    actor: actor.clone(),
                    action: "privilege.protocol_upsert".into(),
                    entity: format!("matter:{}", self.id()),
                    params_json,
                    tool_version: env!("CARGO_PKG_VERSION").into(),
                },
                &now,
            )?;
            Ok(())
        })?;
        self.get_privilege_protocol()
    }

    /// Production hold: `EXISTS item_privilege WHERE withhold = 1`.
    ///
    /// **0040** must call this (or [`Self::list_withheld_item_ids`]) before
    /// natives / load-file production.
    pub fn item_is_withheld(&self, item_id: &str) -> Result<bool> {
        self.ensure_item_in_matter(item_id)?;
        let n: i64 = self.connection().query_row(
            "SELECT COUNT(*) FROM item_privilege \
             WHERE item_id = ?1 AND matter_id = ?2 AND withhold = 1",
            params![item_id, self.id()],
            |row| row.get(0),
        )?;
        Ok(n > 0)
    }

    /// All item ids in this matter with `withhold = 1` (sorted).
    pub fn list_withheld_item_ids(&self) -> Result<Vec<String>> {
        let mut stmt = self.connection().prepare(
            "SELECT item_id FROM item_privilege \
             WHERE matter_id = ?1 AND withhold = 1 \
             ORDER BY item_id ASC",
        )?;
        let rows = stmt.query_map(params![self.id()], |row| row.get::<_, String>(0))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// Family unit privilege split check (parent + children / same family_id).
    ///
    /// Privileged := active privilege status OR has `privilege` code.
    pub fn family_privilege_consistency(
        &self,
        item_id: &str,
    ) -> Result<FamilyPrivilegeConsistency> {
        self.ensure_item_in_matter(item_id)?;
        let mut members = self.expand_family_units(&[item_id.to_string()])?;
        members.sort();
        members.dedup();
        let mut privileged_ids = Vec::new();
        let mut non_privileged_ids = Vec::new();
        for id in members {
            if item_is_privileged_for_family(self.connection(), self.id(), &id)? {
                privileged_ids.push(id);
            } else {
                non_privileged_ids.push(id);
            }
        }
        // Split only when both sides non-empty within a multi-member family.
        let consistent = privileged_ids.is_empty() || non_privileged_ids.is_empty();
        Ok(FamilyPrivilegeConsistency {
            consistent,
            privileged_ids,
            non_privileged_ids,
        })
    }

    /// Export standard privilege log CSV (UTF-8, RFC4180, header row).
    ///
    /// Eligibility: `include_on_log=1` AND status ∈ asserted/under_review/partial_redaction.
    /// Cleared rows never appear. Blank descriptions are exported with a warning count.
    /// Attachment rows inherit empty From/To/Cc/Bcc/Subject/DocDate from parent email.
    pub fn export_privilege_log(
        &self,
        params: PrivilegeLogExportParams,
    ) -> Result<PrivilegeLogExportResult> {
        let scope = params.scope.trim();
        if scope != SCOPE_REVIEW_CORPUS && scope != SCOPE_ENTIRE_MATTER {
            return Err(Error::Other(format!(
                "invalid privilege log scope '{scope}'; expected \
                 '{SCOPE_REVIEW_CORPUS}' or '{SCOPE_ENTIRE_MATTER}'"
            )));
        }
        let export_path = params.path;
        if let Some(parent) = export_path.parent() {
            if !parent.as_str().is_empty() {
                fs::create_dir_all(parent.as_std_path())?;
            }
        }

        let exported_at = now_rfc3339();
        let matter_id = self.id().to_string();

        // Eligible privilege rows joined to items, sorted sent_at NULLS LAST, path, id.
        let mut sql = String::from(
            "SELECT \
                i.id, i.parent_item_id, i.family_id, i.custodian, \
                i.sent_at, i.received_at, i.created_at, \
                i.from_addr, i.to_addrs_json, i.cc_addrs_json, i.bcc_addrs_json, \
                i.subject, i.title, i.path, i.file_category, i.mime_type, i.in_review, \
                p.basis, p.description, p.status, p.withhold \
             FROM item_privilege p \
             INNER JOIN items i ON i.id = p.item_id AND i.matter_id = p.matter_id \
             WHERE p.matter_id = ?1 \
               AND p.include_on_log = 1 \
               AND p.status IN ('asserted', 'under_review', 'partial_redaction')",
        );
        let mut bind: Vec<Value> = vec![Value::Text(matter_id.clone())];
        if scope == SCOPE_REVIEW_CORPUS {
            sql.push_str(" AND i.in_review = 1");
        }
        if let Some(ref ids) = params.filter_ids {
            if !ids.is_empty() {
                let ph = ids.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
                sql.push_str(&format!(" AND i.id IN ({ph})"));
                for id in ids {
                    bind.push(Value::Text(id.clone()));
                }
            }
        }
        // sent_at ASC NULLS LAST, path, id
        sql.push_str(
            " ORDER BY (i.sent_at IS NULL), i.sent_at ASC, \
              i.path ASC, i.id ASC",
        );

        let mut stmt = self.connection().prepare(&sql)?;
        let rows_iter = stmt.query_map(params_from_iter(bind), |row| {
            Ok(ExportItemRow {
                id: row.get(0)?,
                parent_item_id: row.get(1)?,
                family_id: row.get(2)?,
                custodian: row.get(3)?,
                sent_at: row.get(4)?,
                received_at: row.get(5)?,
                created_at: row.get(6)?,
                from_addr: row.get(7)?,
                to_addrs_json: row.get(8)?,
                cc_addrs_json: row.get(9)?,
                bcc_addrs_json: row.get(10)?,
                subject: row.get(11)?,
                title: row.get(12)?,
                path: row.get(13)?,
                file_category: row.get(14)?,
                mime_type: row.get(15)?,
                in_review: row.get(16)?,
                basis: row.get(17)?,
                description: row.get(18)?,
                status: row.get(19)?,
                withhold: row.get(20)?,
            })
        })?;

        let mut export_rows: Vec<ExportItemRow> = Vec::new();
        for r in rows_iter {
            export_rows.push(r?);
        }

        // Batch parent lookup (no N+1).
        let parent_ids: HashSet<String> = export_rows
            .iter()
            .filter_map(|r| r.parent_item_id.clone())
            .collect();
        let parents = load_parent_meta(self.connection(), self.id(), &parent_ids)?;

        // Privilege code presence for HasPrivilegeCode column.
        let item_ids: Vec<String> = export_rows.iter().map(|r| r.id.clone()).collect();
        let has_priv_code = items_with_privilege_code(self.connection(), self.id(), &item_ids)?;

        let mut blank_description_count = 0u64;
        let mut withheld_count = 0u64;
        let mut csv = String::new();
        csv.push_str(&PRIVILEGE_LOG_COLUMNS.join(","));
        csv.push('\n');

        for row in &export_rows {
            let parent = row
                .parent_item_id
                .as_ref()
                .and_then(|pid| parents.get(pid.as_str()));

            let from = first_nonempty(
                row.from_addr.as_deref(),
                parent.and_then(|p| p.from_addr.as_deref()),
            );
            let to_own = join_addrs_json(row.to_addrs_json.as_deref());
            let to = if to_own.is_empty() {
                parent
                    .map(|p| join_addrs_json(p.to_addrs_json.as_deref()))
                    .unwrap_or_default()
            } else {
                to_own
            };
            let cc_own = join_addrs_json(row.cc_addrs_json.as_deref());
            let cc = if cc_own.is_empty() {
                parent
                    .map(|p| join_addrs_json(p.cc_addrs_json.as_deref()))
                    .unwrap_or_default()
            } else {
                cc_own
            };
            let bcc_own = join_addrs_json(row.bcc_addrs_json.as_deref());
            let bcc = if bcc_own.is_empty() {
                parent
                    .map(|p| join_addrs_json(p.bcc_addrs_json.as_deref()))
                    .unwrap_or_default()
            } else {
                bcc_own
            };
            let subject = first_nonempty(
                row.subject.as_deref().or(row.title.as_deref()),
                parent.and_then(|p| p.subject.as_deref().or(p.title.as_deref())),
            );
            let doc_date_item = doc_date_from(
                row.sent_at.as_deref(),
                row.received_at.as_deref(),
                row.created_at.as_deref(),
            );
            let doc_date = if doc_date_item.is_empty() {
                parent
                    .map(|p| {
                        doc_date_from(
                            p.sent_at.as_deref(),
                            p.received_at.as_deref(),
                            p.created_at.as_deref(),
                        )
                    })
                    .unwrap_or_default()
            } else {
                doc_date_item
            };
            let custodian = first_nonempty(
                row.custodian.as_deref(),
                parent.and_then(|p| p.custodian.as_deref()),
            );
            let file_name = path_basename(row.path.as_deref());
            let file_type = first_nonempty(row.file_category.as_deref(), row.mime_type.as_deref());
            let priv_type = basis_label(&row.basis);
            let desc = row.description.clone();
            if desc.trim().is_empty() {
                blank_description_count += 1;
            }
            let withhold_y = row.withhold == 1;
            if withhold_y {
                withheld_count += 1;
            }
            let has_code = has_priv_code.contains(&row.id);

            let fields = [
                row.id.as_str(),
                row.parent_item_id.as_deref().unwrap_or(""),
                row.family_id.as_deref().unwrap_or(""),
                custodian.as_str(),
                doc_date.as_str(),
                from.as_str(),
                to.as_str(),
                cc.as_str(),
                bcc.as_str(),
                subject.as_str(),
                file_name.as_str(),
                file_type.as_str(),
                priv_type,
                desc.as_str(),
                row.status.as_str(),
                yn(withhold_y),
                yn(has_code),
                matter_id.as_str(),
                exported_at.as_str(),
            ];
            let line = fields
                .iter()
                .map(|f| csv_escape_field(f))
                .collect::<Vec<_>>()
                .join(",");
            csv.push_str(&line);
            csv.push('\n');
        }

        let row_count = export_rows.len() as u64;

        // Write file (atomic-ish: write then done).
        {
            let mut f = fs::File::create(export_path.as_std_path())?;
            f.write_all(csv.as_bytes())?;
            f.flush()?;
        }

        let result = PrivilegeLogExportResult {
            path: export_path.to_string(),
            row_count,
            blank_description_count,
            withheld_count,
        };

        let actor = "desk".to_string();
        let now = now_rfc3339();
        let params_json = serde_json::json!({
            "path": result.path,
            "scope": scope,
            "row_count": row_count,
            "blank_description_count": blank_description_count,
            "withheld_count": withheld_count,
            "filter_ids": params.filter_ids,
        })
        .to_string();
        audit::append_event(
            self.connection(),
            &AuditEventInput {
                actor,
                action: "privilege.log_export".into(),
                entity: format!("matter:{matter_id}"),
                params_json,
                tool_version: env!("CARGO_PKG_VERSION").into(),
            },
            &now,
        )?;

        Ok(result)
    }

    /// Append batch privilege.upsert audit (full sorted item_ids) — coding hook.
    pub(crate) fn audit_privilege_batch_upsert(
        conn: &Connection,
        actor: &str,
        item_ids: &[String],
        now: &str,
    ) -> Result<()> {
        if item_ids.is_empty() {
            return Ok(());
        }
        let mut ids = item_ids.to_vec();
        ids.sort();
        ids.dedup();
        let entity = if ids.len() == 1 {
            format!("item:{}", ids[0])
        } else {
            "batch".to_string()
        };
        let params_json = serde_json::json!({
            "item_ids": ids,
            "op": "ensure",
            "source": "coding.apply",
        })
        .to_string();
        audit::append_event(
            conn,
            &AuditEventInput {
                actor: actor.to_string(),
                action: "privilege.upsert".into(),
                entity,
                params_json,
                tool_version: env!("CARGO_PKG_VERSION").into(),
            },
            now,
        )?;
        Ok(())
    }

    /// Append batch privilege.clear audit (full sorted item_ids) — coding hook.
    pub(crate) fn audit_privilege_batch_clear(
        conn: &Connection,
        actor: &str,
        item_ids: &[String],
        now: &str,
    ) -> Result<()> {
        if item_ids.is_empty() {
            return Ok(());
        }
        let mut ids = item_ids.to_vec();
        ids.sort();
        ids.dedup();
        let entity = if ids.len() == 1 {
            format!("item:{}", ids[0])
        } else {
            "batch".to_string()
        };
        let params_json = serde_json::json!({
            "item_ids": ids,
            "op": "clear",
            "source": "coding.apply",
            "description_retained": true,
        })
        .to_string();
        audit::append_event(
            conn,
            &AuditEventInput {
                actor: actor.to_string(),
                action: "privilege.clear".into(),
                entity,
                params_json,
                tool_version: env!("CARGO_PKG_VERSION").into(),
            },
            now,
        )?;
        Ok(())
    }
}

fn load_privilege(
    conn: &Connection,
    matter_id: &str,
    item_id: &str,
) -> Result<Option<ItemPrivilege>> {
    conn.query_row(
        &format!(
            "SELECT {PRIVILEGE_SELECT} FROM item_privilege WHERE item_id = ?1 AND matter_id = ?2"
        ),
        params![item_id, matter_id],
        map_privilege_row,
    )
    .optional()
    .map_err(Error::from)
}

#[derive(Debug)]
struct ExportItemRow {
    id: String,
    parent_item_id: Option<String>,
    family_id: Option<String>,
    custodian: Option<String>,
    sent_at: Option<String>,
    received_at: Option<String>,
    created_at: Option<String>,
    from_addr: Option<String>,
    to_addrs_json: Option<String>,
    cc_addrs_json: Option<String>,
    bcc_addrs_json: Option<String>,
    subject: Option<String>,
    title: Option<String>,
    path: Option<String>,
    file_category: Option<String>,
    mime_type: Option<String>,
    #[allow(dead_code)]
    in_review: Option<i64>,
    basis: String,
    description: String,
    status: String,
    withhold: i64,
}

#[derive(Debug, Clone)]
struct ParentMeta {
    custodian: Option<String>,
    sent_at: Option<String>,
    received_at: Option<String>,
    created_at: Option<String>,
    from_addr: Option<String>,
    to_addrs_json: Option<String>,
    cc_addrs_json: Option<String>,
    bcc_addrs_json: Option<String>,
    subject: Option<String>,
    title: Option<String>,
}

fn load_parent_meta(
    conn: &Connection,
    matter_id: &str,
    parent_ids: &HashSet<String>,
) -> Result<HashMap<String, ParentMeta>> {
    let mut out = HashMap::new();
    if parent_ids.is_empty() {
        return Ok(out);
    }
    let ids: Vec<String> = parent_ids.iter().cloned().collect();
    for chunk in ids.chunks(200) {
        let ph = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
        let sql = format!(
            "SELECT id, custodian, sent_at, received_at, created_at, from_addr, \
                    to_addrs_json, cc_addrs_json, bcc_addrs_json, subject, title \
             FROM items WHERE matter_id = ? AND id IN ({ph})"
        );
        let mut params: Vec<Value> = vec![Value::Text(matter_id.to_string())];
        for id in chunk {
            params.push(Value::Text(id.clone()));
        }
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(params), |row| {
            Ok((
                row.get::<_, String>(0)?,
                ParentMeta {
                    custodian: row.get(1)?,
                    sent_at: row.get(2)?,
                    received_at: row.get(3)?,
                    created_at: row.get(4)?,
                    from_addr: row.get(5)?,
                    to_addrs_json: row.get(6)?,
                    cc_addrs_json: row.get(7)?,
                    bcc_addrs_json: row.get(8)?,
                    subject: row.get(9)?,
                    title: row.get(10)?,
                },
            ))
        })?;
        for r in rows {
            let (id, meta) = r?;
            out.insert(id, meta);
        }
    }
    Ok(out)
}

fn items_with_privilege_code(
    conn: &Connection,
    matter_id: &str,
    item_ids: &[String],
) -> Result<HashSet<String>> {
    let mut out = HashSet::new();
    if item_ids.is_empty() {
        return Ok(out);
    }
    for chunk in item_ids.chunks(200) {
        let ph = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
        let sql = format!(
            "SELECT ic.item_id FROM item_codes ic \
             JOIN code_definitions cd ON cd.id = ic.code_id \
             WHERE cd.matter_id = ? AND cd.key = 'privilege' \
               AND ic.item_id IN ({ph})"
        );
        let mut params: Vec<Value> = vec![Value::Text(matter_id.to_string())];
        for id in chunk {
            params.push(Value::Text(id.clone()));
        }
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(params), |row| row.get::<_, String>(0))?;
        for r in rows {
            out.insert(r?);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basis_labels_cover_vocab() {
        for b in privilege_basis::ALL {
            let label = basis_label(b);
            assert!(!label.is_empty());
        }
    }

    #[test]
    fn csv_escape_quotes_commas() {
        assert_eq!(csv_escape_field("plain"), "plain");
        assert_eq!(csv_escape_field("a,b"), "\"a,b\"");
        assert_eq!(csv_escape_field("say \"hi\""), "\"say \"\"hi\"\"\"");
    }

    #[test]
    fn join_addrs_semicolon() {
        assert_eq!(
            join_addrs_json(Some(r#"["a@x.com","b@y.com"]"#)),
            "a@x.com; b@y.com"
        );
        assert_eq!(join_addrs_json(None), "");
    }

    #[test]
    fn path_basename_works() {
        assert_eq!(path_basename(Some("inbox/att/foo.pdf")), "foo.pdf");
        assert_eq!(path_basename(Some(r"inbox\att\bar.docx")), "bar.docx");
    }

    #[test]
    fn privilege_log_columns_count() {
        assert_eq!(PRIVILEGE_LOG_COLUMNS.len(), 19);
        assert_eq!(PRIVILEGE_LOG_COLUMNS[0], "ControlNumber");
        assert_eq!(PRIVILEGE_LOG_COLUMNS[12], "PrivilegeType");
    }
}
