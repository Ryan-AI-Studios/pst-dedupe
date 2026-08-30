//! Geometric redaction regions + burned-native bookkeeping (schema v40 / track 0114).
//!
//! Stand-off PDF user-space (y-up) rects, **separate** from `item_redactions`
//! (text char ranges). Original `native_sha256` CAS is never rewritten; burn
//! writes a new CAS blob recorded on `items.burned_native_sha256`.

use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::audit::{self, AuditEventInput};
use crate::cas::sha256_hex;
use crate::error::{Error, Result};
use crate::matter::{new_id, normalize_actor, now_rfc3339, Item, Matter};
use crate::redaction::{redaction_reason, redaction_status};

/// Engine pin folded into the burn fingerprint (CHANGELOG records the crate pin).
pub const RASTER_ENGINE_PIN: &str = "zpdf-0.13.0";

/// Value stored on `items.raster_engine` after a successful zpdf burn.
pub const RASTER_ENGINE_ZPDF: &str = "zpdf";

/// Geometric-box source vocabulary.
pub mod geom_source {
    pub const DRAW: &str = "draw";
    pub const HIT: &str = "hit";
    pub const FULL_PAGE: &str = "full_page";

    pub const ALL: &[&str] = &[DRAW, HIT, FULL_PAGE];
}

/// Draft geometric redaction region (schema v40). Coordinates are PDF user space
/// (y-up, origin bottom-left). For JPEG/PNG natives the same fields hold pixel
/// space (y-down origin top-left) as stored by the host.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ItemGeomRedaction {
    pub id: String,
    pub item_id: String,
    pub matter_id: String,
    /// 0-based page index.
    pub page_index: i64,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    /// `privilege` | `pii` | `confidential` | `other`.
    pub reason: String,
    pub label: Option<String>,
    /// `active` or `stale`.
    pub status: String,
    /// `draw` | `hit` | `full_page`.
    pub source: String,
    pub created_at: String,
    pub updated_at: String,
    pub created_by: Option<String>,
}

/// Input for [`Matter::create_geom_redaction`].
#[derive(Debug, Clone)]
pub struct CreateGeomRedactionInput {
    pub item_id: String,
    pub page_index: i64,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub reason: String,
    pub label: Option<String>,
    pub source: String,
    pub actor: String,
}

/// Persisted burned-native pointers after a successful engine burn.
#[derive(Debug, Clone)]
pub struct SetBurnedNativeInput {
    pub item_id: String,
    pub burned_native_sha256: String,
    /// Fingerprint of native+geom+text at burn start; must still match at persist.
    pub expected_fingerprint: String,
    pub actor: String,
}

fn validate_reason(reason: &str) -> Result<()> {
    if redaction_reason::ALL.contains(&reason) {
        Ok(())
    } else {
        Err(Error::Other(format!(
            "invalid redaction reason '{reason}'; expected one of: {}",
            redaction_reason::ALL.join(", ")
        )))
    }
}

fn validate_source(source: &str) -> Result<()> {
    if geom_source::ALL.contains(&source) {
        Ok(())
    } else {
        Err(Error::Other(format!(
            "invalid geom source '{source}'; expected one of: {}",
            geom_source::ALL.join(", ")
        )))
    }
}

fn map_geom_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ItemGeomRedaction> {
    Ok(ItemGeomRedaction {
        id: row.get(0)?,
        item_id: row.get(1)?,
        matter_id: row.get(2)?,
        page_index: row.get(3)?,
        x: row.get(4)?,
        y: row.get(5)?,
        w: row.get(6)?,
        h: row.get(7)?,
        reason: row.get(8)?,
        label: row.get(9)?,
        status: row.get(10)?,
        source: row.get(11)?,
        created_at: row.get(12)?,
        updated_at: row.get(13)?,
        created_by: row.get(14)?,
    })
}

const GEOM_SELECT: &str = "id, item_id, matter_id, page_index, x, y, w, h, reason, label, \
    status, source, created_at, updated_at, created_by";

/// `%PDF-` after optional UTF-8 BOM / leading whitespace.
pub fn bytes_look_like_pdf(bytes: &[u8]) -> bool {
    let mut i = 0usize;
    if bytes.len() >= 3 && bytes[0] == 0xEF && bytes[1] == 0xBB && bytes[2] == 0xBF {
        i = 3;
    }
    while i < bytes.len() && matches!(bytes[i], b' ' | b'\t' | b'\r' | b'\n') {
        i += 1;
    }
    let rest = &bytes[i..];
    rest.len() >= 5 && rest.starts_with(b"%PDF-")
}

/// Path / mime / category look like a PDF (no byte sniff).
pub fn item_looks_like_pdf(item: &Item) -> bool {
    if let Some(path) = item.path.as_deref() {
        let lower = path.to_ascii_lowercase();
        let leaf = lower.rsplit(['/', '\\', '!']).next().unwrap_or(&lower);
        if leaf.ends_with(".pdf") {
            return true;
        }
    }
    if let Some(mime) = item.mime_type.as_deref() {
        let m = mime.to_ascii_lowercase();
        if m == "application/pdf" || m.starts_with("application/pdf;") {
            return true;
        }
    }
    matches!(
        item.file_category.as_deref().map(|c| c.to_ascii_lowercase()),
        Some(ref c) if c == "pdf"
    )
}

/// Metadata first; if redactions exist and metadata is not PDF, sniff native magic.
pub fn item_is_pdf_native(matter: &Matter, item: &Item) -> Result<bool> {
    if item_looks_like_pdf(item) {
        return Ok(true);
    }
    if item.redaction_count <= 0 && item.geom_redaction_count <= 0 {
        return Ok(false);
    }
    let Some(sha) = item
        .native_sha256
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    else {
        return Ok(false);
    };
    let head = matter.read_cas_prefix(sha, 64)?;
    Ok(bytes_look_like_pdf(&head))
}

fn digest_present(d: Option<&str>) -> bool {
    d.map(|s| !s.trim().is_empty()).unwrap_or(false)
}

fn canonical_geom_line(g: &ItemGeomRedaction) -> String {
    format!(
        "{};{:.12};{:.12};{:.12};{:.12};{}",
        g.page_index, g.x, g.y, g.w, g.h, g.id
    )
}

/// SHA-256 hex of native digest + canonical active geom + 0032 text state + engine pin.
pub fn compute_burn_fingerprint(
    native_sha256: Option<&str>,
    active_geom: &[ItemGeomRedaction],
    redacted_text_sha256: Option<&str>,
    active_text_count: u64,
    active_text_max_updated: Option<&str>,
) -> String {
    let mut geoms: Vec<&ItemGeomRedaction> = active_geom.iter().collect();
    geoms.sort_by(|a, b| {
        a.page_index
            .cmp(&b.page_index)
            .then(a.x.total_cmp(&b.x))
            .then(a.y.total_cmp(&b.y))
            .then(a.w.total_cmp(&b.w))
            .then(a.h.total_cmp(&b.h))
            .then(a.id.cmp(&b.id))
    });
    let sha = redacted_text_sha256
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("");
    let text_state = format!(
        "redacted_text_sha256={sha};count={active_text_count};max_updated={}",
        active_text_max_updated.unwrap_or("")
    );
    let mut preimage = String::new();
    preimage.push_str("native=");
    preimage.push_str(native_sha256.map(str::trim).unwrap_or(""));
    preimage.push('\n');
    preimage.push_str("geom=\n");
    for g in geoms {
        preimage.push_str(&canonical_geom_line(g));
        preimage.push('\n');
    }
    preimage.push_str("text=");
    preimage.push_str(&text_state);
    preimage.push('\n');
    preimage.push_str("engine=");
    preimage.push_str(RASTER_ENGINE_PIN);
    preimage.push('\n');
    sha256_hex(preimage.as_bytes())
}

/// Whether produce must copy `burned_native_sha256` (never the original native).
pub fn burn_required(item: &Item, current_fingerprint: &str) -> bool {
    burn_required_pdf_known(item, current_fingerprint, item_looks_like_pdf(item))
}

/// Same as [`burn_required`] with an explicit PDF-native verdict (byte sniff).
pub fn burn_required_pdf_known(item: &Item, current_fingerprint: &str, is_pdf: bool) -> bool {
    let content = item.geom_redaction_count > 0 || (is_pdf && item.redaction_count > 0);
    if content {
        return true;
    }
    matches!(
        item.burned_source_digest
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty()),
        Some(stored) if stored != current_fingerprint
    )
}

/// Fresh burned native: digest present and fingerprint matches.
pub fn burned_native_fresh(item: &Item, current_fingerprint: &str) -> bool {
    digest_present(item.burned_native_sha256.as_deref())
        && item
            .burned_source_digest
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            == Some(current_fingerprint)
}

impl Matter {
    /// List geom redactions for an item (page then create order).
    pub fn list_geom_redactions(&self, item_id: &str) -> Result<Vec<ItemGeomRedaction>> {
        self.ensure_item_in_matter(item_id)?;
        let mut stmt = self.connection().prepare(&format!(
            "SELECT {GEOM_SELECT} FROM item_geom_redactions \
             WHERE item_id = ?1 AND matter_id = ?2 \
             ORDER BY page_index ASC, created_at ASC, id ASC"
        ))?;
        let rows = stmt.query_map(params![item_id, self.id()], map_geom_row)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Error::from)
    }

    /// Active geom rows only.
    pub fn list_active_geom_redactions(&self, item_id: &str) -> Result<Vec<ItemGeomRedaction>> {
        self.ensure_item_in_matter(item_id)?;
        let mut stmt = self.connection().prepare(&format!(
            "SELECT {GEOM_SELECT} FROM item_geom_redactions \
             WHERE item_id = ?1 AND matter_id = ?2 AND status = ?3 \
             ORDER BY page_index ASC, created_at ASC, id ASC"
        ))?;
        let rows = stmt.query_map(
            params![item_id, self.id(), redaction_status::ACTIVE],
            map_geom_row,
        )?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Error::from)
    }

    /// Load one geom redaction by id.
    pub fn get_geom_redaction(&self, geom_id: &str) -> Result<ItemGeomRedaction> {
        self.connection()
            .query_row(
                &format!("SELECT {GEOM_SELECT} FROM item_geom_redactions WHERE id = ?1"),
                params![geom_id],
                map_geom_row,
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    Error::Other(format!("geom redaction not found: {geom_id}"))
                }
                other => Error::Sqlite(other),
            })
    }

    /// Create a geometric redaction. Bumps `geom_redaction_count`, audits.
    pub fn create_geom_redaction(
        &self,
        input: CreateGeomRedactionInput,
    ) -> Result<ItemGeomRedaction> {
        let actor = normalize_actor(&input.actor);
        self.ensure_item_in_matter(&input.item_id)?;

        let reason = input.reason.trim().to_string();
        validate_reason(&reason)?;
        let source = input.source.trim().to_string();
        validate_source(&source)?;

        if input.page_index < 0 {
            return Err(Error::Other(
                "geom page_index must be >= 0 (0-based)".into(),
            ));
        }
        if !(input.w > 0.0 && input.h > 0.0 && input.w.is_finite() && input.h.is_finite()) {
            return Err(Error::Other("geom w and h must be finite and > 0".into()));
        }
        if !input.x.is_finite() || !input.y.is_finite() {
            return Err(Error::Other("geom x and y must be finite".into()));
        }

        let label = input
            .label
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());

        let id = new_id("gxr");
        let now = now_rfc3339();
        let params_json = serde_json::json!({
            "geom_id": id,
            "item_id": input.item_id,
            "page_index": input.page_index,
            "x": input.x,
            "y": input.y,
            "w": input.w,
            "h": input.h,
            "reason": reason,
            "label": label,
            "source": source,
        })
        .to_string();

        self.with_transaction(|conn| {
            conn.execute(
                "INSERT INTO item_geom_redactions \
                 (id, item_id, matter_id, page_index, x, y, w, h, reason, label, \
                  status, source, created_at, updated_at, created_by) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
                params![
                    id,
                    input.item_id,
                    self.id(),
                    input.page_index,
                    input.x,
                    input.y,
                    input.w,
                    input.h,
                    reason,
                    label,
                    redaction_status::ACTIVE,
                    source,
                    now,
                    now,
                    actor
                ],
            )?;
            conn.execute(
                "UPDATE items SET geom_redaction_count = geom_redaction_count + 1 \
                 WHERE id = ?1 AND matter_id = ?2",
                params![input.item_id, self.id()],
            )?;
            audit::append_event(
                conn,
                &AuditEventInput {
                    actor: actor.clone(),
                    action: "geom_redaction.create".into(),
                    entity: format!("geom_redaction:{id}"),
                    params_json: params_json.clone(),
                    tool_version: env!("CARGO_PKG_VERSION").into(),
                },
                &now,
            )?;
            Ok(())
        })?;

        self.get_geom_redaction(&id)
    }

    /// Hard-delete a geom region. Decrements count when the row was active. Audit retains the rect.
    pub fn delete_geom_redaction(&self, geom_id: &str, actor: &str) -> Result<()> {
        let actor = normalize_actor(actor);
        let existing = self.get_geom_redaction(geom_id)?;
        if existing.matter_id != self.id() {
            return Err(Error::Other(format!(
                "geom redaction {geom_id} belongs to another matter"
            )));
        }
        let now = now_rfc3339();
        let was_active = existing.status == redaction_status::ACTIVE;
        let params_json = serde_json::json!({
            "geom_id": geom_id,
            "item_id": existing.item_id,
            "page_index": existing.page_index,
            "x": existing.x,
            "y": existing.y,
            "w": existing.w,
            "h": existing.h,
            "reason": existing.reason,
            "source": existing.source,
            "status": existing.status,
        })
        .to_string();

        self.with_transaction(|conn| {
            conn.execute(
                "DELETE FROM item_geom_redactions WHERE id = ?1",
                params![geom_id],
            )?;
            if was_active {
                conn.execute(
                    "UPDATE items SET geom_redaction_count = MAX(0, geom_redaction_count - 1) \
                     WHERE id = ?1 AND matter_id = ?2",
                    params![existing.item_id, self.id()],
                )?;
            }
            audit::append_event(
                conn,
                &AuditEventInput {
                    actor: actor.clone(),
                    action: "geom_redaction.delete".into(),
                    entity: format!("geom_redaction:{geom_id}"),
                    params_json: params_json.clone(),
                    tool_version: env!("CARGO_PKG_VERSION").into(),
                },
                &now,
            )?;
            Ok(())
        })?;
        Ok(())
    }

    fn text_redaction_state(&self, item_id: &str) -> Result<(u64, Option<String>)> {
        let mut stmt = self.connection().prepare(
            "SELECT COUNT(*), MAX(updated_at) FROM item_redactions \
             WHERE item_id = ?1 AND matter_id = ?2 AND status = ?3",
        )?;
        stmt.query_row(
            params![item_id, self.id(), redaction_status::ACTIVE],
            |row| {
                let count: i64 = row.get(0)?;
                let max_updated: Option<String> = row.get(1)?;
                Ok((count.max(0) as u64, max_updated))
            },
        )
        .map_err(Error::from)
    }

    /// Current burn fingerprint for an item (native + active geom + 0032 text + pin).
    pub fn geom_burn_fingerprint(&self, item_id: &str) -> Result<String> {
        let item = self.get_item(item_id)?;
        let active = self.list_active_geom_redactions(item_id)?;
        let (text_count, text_max) = self.text_redaction_state(item_id)?;
        Ok(compute_burn_fingerprint(
            item.native_sha256.as_deref(),
            &active,
            item.redacted_text_sha256.as_deref(),
            text_count,
            text_max.as_deref(),
        ))
    }

    /// Whether produce/QC must require a fresh burned native for this item.
    pub fn item_burn_required(&self, item: &Item) -> Result<bool> {
        let fp = self.geom_burn_fingerprint(&item.id)?;
        let is_pdf = item_is_pdf_native(self, item)?;
        Ok(burn_required_pdf_known(item, &fp, is_pdf))
    }

    /// Persist burned digest + fingerprint after a successful engine burn. Audits.
    pub fn set_burned_native(&self, input: SetBurnedNativeInput) -> Result<Item> {
        let actor = normalize_actor(&input.actor);
        self.ensure_item_in_matter(&input.item_id)?;
        let sha = input.burned_native_sha256.trim();
        if sha.is_empty() {
            return Err(Error::Other("burned_native_sha256 cannot be empty".into()));
        }
        let item = self.get_item(&input.item_id)?;
        if item
            .native_sha256
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            == Some(sha)
        {
            return Err(Error::Other(
                "burned_native_sha256 must differ from original native_sha256".into(),
            ));
        }
        if !self.blob_exists(sha)? {
            return Err(Error::Other(format!(
                "burned native CAS blob not found: {sha}"
            )));
        }
        let fp = self.geom_burn_fingerprint(&input.item_id)?;
        if fp != input.expected_fingerprint.trim() {
            return Err(Error::Other(
                "burn snapshot stale: native or geom/text changed during burn".into(),
            ));
        }
        let now = now_rfc3339();
        let params_json = serde_json::json!({
            "item_id": input.item_id,
            "burned_native_sha256": sha,
            "burned_source_digest": fp,
            "raster_engine": RASTER_ENGINE_ZPDF,
        })
        .to_string();

        self.with_transaction(|conn| {
            conn.execute(
                "UPDATE items SET burned_native_sha256 = ?1, burned_native_at = ?2, \
                        burned_source_digest = ?3, raster_engine = ?4 \
                 WHERE id = ?5 AND matter_id = ?6",
                params![sha, now, fp, RASTER_ENGINE_ZPDF, input.item_id, self.id()],
            )?;
            audit::append_event(
                conn,
                &AuditEventInput {
                    actor: actor.clone(),
                    action: "native.burn".into(),
                    entity: format!("item:{}", input.item_id),
                    params_json: params_json.clone(),
                    tool_version: env!("CARGO_PKG_VERSION").into(),
                },
                &now,
            )?;
            Ok(())
        })?;
        self.get_item(&input.item_id)
    }
}
