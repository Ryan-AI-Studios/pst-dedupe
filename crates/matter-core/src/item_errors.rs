//! Item-level error accumulator for honest partial success.
//!
//! Failures are **recorded**; parent `items` rows remain.

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// A recorded item-level (or source/job-level) processing error.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemError {
    pub id: i64,
    pub item_id: Option<String>,
    pub source_id: Option<String>,
    pub job_id: Option<String>,
    pub stage: String,
    pub code: String,
    pub message: String,
    pub detail: Option<String>,
    pub created_at: String,
}

/// Input for recording an item error.
#[derive(Debug, Clone)]
pub struct ItemErrorInput {
    pub item_id: Option<String>,
    pub source_id: Option<String>,
    pub job_id: Option<String>,
    pub stage: String,
    pub code: String,
    pub message: String,
    pub detail: Option<String>,
}

pub(crate) fn record(conn: &Connection, input: &ItemErrorInput, now: &str) -> Result<ItemError> {
    if let Some(ref item_id) = input.item_id {
        ensure_item_exists(conn, item_id)?;
    }
    if let Some(ref source_id) = input.source_id {
        ensure_source_exists(conn, source_id)?;
    }
    if let Some(ref job_id) = input.job_id {
        ensure_job_exists(conn, job_id)?;
    }

    conn.execute(
        "INSERT INTO item_errors (item_id, source_id, job_id, stage, code, message, detail, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            input.item_id,
            input.source_id,
            input.job_id,
            input.stage,
            input.code,
            input.message,
            input.detail,
            now,
        ],
    )?;
    let id = conn.last_insert_rowid();
    get_by_id(conn, id)?.ok_or_else(|| Error::Other("item_error missing after insert".into()))
}

pub(crate) fn get_by_id(conn: &Connection, id: i64) -> Result<Option<ItemError>> {
    conn.query_row(
        "SELECT id, item_id, source_id, job_id, stage, code, message, detail, created_at \
         FROM item_errors WHERE id = ?1",
        params![id],
        map_row,
    )
    .optional()
    .map_err(Error::from)
}

pub(crate) fn for_item(conn: &Connection, item_id: &str) -> Result<Vec<ItemError>> {
    query_list(
        conn,
        "SELECT id, item_id, source_id, job_id, stage, code, message, detail, created_at \
         FROM item_errors WHERE item_id = ?1 ORDER BY id ASC",
        params![item_id],
    )
}

pub(crate) fn for_source(conn: &Connection, source_id: &str) -> Result<Vec<ItemError>> {
    query_list(
        conn,
        "SELECT id, item_id, source_id, job_id, stage, code, message, detail, created_at \
         FROM item_errors WHERE source_id = ?1 ORDER BY id ASC",
        params![source_id],
    )
}

pub(crate) fn for_job(conn: &Connection, job_id: &str) -> Result<Vec<ItemError>> {
    query_list(
        conn,
        "SELECT id, item_id, source_id, job_id, stage, code, message, detail, created_at \
         FROM item_errors WHERE job_id = ?1 ORDER BY id ASC",
        params![job_id],
    )
}

fn map_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ItemError> {
    Ok(ItemError {
        id: row.get(0)?,
        item_id: row.get(1)?,
        source_id: row.get(2)?,
        job_id: row.get(3)?,
        stage: row.get(4)?,
        code: row.get(5)?,
        message: row.get(6)?,
        detail: row.get(7)?,
        created_at: row.get(8)?,
    })
}

fn query_list(
    conn: &Connection,
    sql: &str,
    params: impl rusqlite::Params,
) -> Result<Vec<ItemError>> {
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(params, map_row)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

fn ensure_item_exists(conn: &Connection, item_id: &str) -> Result<()> {
    let exists: bool = conn.query_row(
        "SELECT COUNT(*) > 0 FROM items WHERE id = ?1",
        params![item_id],
        |row| row.get(0),
    )?;
    if !exists {
        return Err(Error::ItemNotFound(item_id.to_string()));
    }
    Ok(())
}

fn ensure_source_exists(conn: &Connection, source_id: &str) -> Result<()> {
    let exists: bool = conn.query_row(
        "SELECT COUNT(*) > 0 FROM sources WHERE id = ?1",
        params![source_id],
        |row| row.get(0),
    )?;
    if !exists {
        return Err(Error::SourceNotFound(source_id.to_string()));
    }
    Ok(())
}

fn ensure_job_exists(conn: &Connection, job_id: &str) -> Result<()> {
    let exists: bool = conn.query_row(
        "SELECT COUNT(*) > 0 FROM jobs WHERE id = ?1",
        params![job_id],
        |row| row.get(0),
    )?;
    if !exists {
        return Err(Error::JobNotFound(job_id.to_string()));
    }
    Ok(())
}
