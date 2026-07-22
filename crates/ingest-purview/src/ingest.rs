//! Matter wiring: detect → source/job → expand → audit → resume.

use camino::Utf8Path;
use matter_core::{AuditEventInput, JobState, Matter};
use serde_json::json;

use crate::detect::{detect, PackageKind};
use crate::error::{Error, Result};
use crate::expand::{expand_package, ExpandCursor, ExpandSession};
use crate::limits::ExpandLimits;

/// Summary returned by successful or partial ingest runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IngestSummary {
    pub source_id: String,
    pub job_id: String,
    pub kind: PackageKind,
    pub entries_ok: u64,
    pub entries_err: u64,
    pub entries_skipped: u64,
    pub bytes_cas: u64,
    pub psts_found: u64,
    pub nested_zips: u64,
    /// True when the run finished fully (job Succeeded).
    pub completed: bool,
    /// True when cancelled mid-run (job Paused; resume-capable).
    pub cancelled: bool,
}

/// Blocking package ingest into an open matter.
///
/// Creates a new `ingest` job, then expands on that job. Prefer
/// [`ingest_path_on_job`] when a process runner already owns job creation.
///
/// **Caller contract:** invoke from a dedicated blocking worker thread
/// (`std::thread`, rayon, or `spawn_blocking`). Do not call on the GUI or
/// Tokio worker threads.
pub fn ingest_path(
    matter: &Matter,
    path: &Utf8Path,
    limits: &ExpandLimits,
    cancel: Option<&dyn Fn() -> bool>,
) -> Result<IngestSummary> {
    if !path.as_std_path().exists() {
        return Err(Error::PackageNotFound(path.to_string()));
    }

    let detected = detect(path)?;
    let is_7z = is_7z_package(path, &detected.notes);

    // Unsupported non-7z: no job row (legacy behaviour).
    if !is_7z && detected.kind == PackageKind::Unsupported {
        return Err(Error::UnsupportedPackage(format!(
            "{} ({})",
            path,
            detected.notes.join("; ")
        )));
    }

    let job = matter.create_job("ingest")?;
    matter.set_job_state(&job.id, JobState::Running, None)?;
    ingest_path_on_job(matter, path, limits, &job.id, cancel)
}

/// Ingest on a **pre-created** job id (Option C — process-runner owns `create_job`).
///
/// Does **not** call `create_job`. Still creates the source row, expands, and
/// finishes job state. Callers must create the job (and typically set Running)
/// before invoking this.
///
/// **Caller contract:** same blocking-thread rules as [`ingest_path`].
pub fn ingest_path_on_job(
    matter: &Matter,
    path: &Utf8Path,
    limits: &ExpandLimits,
    job_id: &str,
    cancel: Option<&dyn Fn() -> bool>,
) -> Result<IngestSummary> {
    if !path.as_std_path().exists() {
        return Err(Error::PackageNotFound(path.to_string()));
    }

    // Ensure the provided job exists and is Running.
    ensure_job_running(matter, job_id)?;

    let detected = detect(path)?;
    let is_7z = is_7z_package(path, &detected.notes);

    // Spec §3.3: `.7z` is out of scope for expand — register source + structured
    // unsupported error on the provided job.
    if is_7z {
        return ingest_unsupported_7z_on_job(matter, path, &detected.notes, job_id);
    }

    if detected.kind == PackageKind::Unsupported {
        return Err(Error::UnsupportedPackage(format!(
            "{} ({})",
            path,
            detected.notes.join("; ")
        )));
    }

    let package_root = path.as_str().to_string();
    let source = matter.insert_source(&package_root, detected.kind.as_str(), "importing", None)?;

    let _ = matter.append_audit(AuditEventInput {
        actor: "system".into(),
        action: "ingest.start".into(),
        entity: format!("source:{}", source.id),
        params_json: json!({
            "path": package_root,
            "kind": detected.kind.as_str(),
            "job_id": job_id,
            "limits": {
                "max_uncompressed_bytes": limits.max_uncompressed_bytes,
                "max_compression_ratio": limits.max_compression_ratio,
                "max_entries": limits.max_entries,
                "max_zip_depth": limits.max_zip_depth,
                "checkpoint_every_n_entries": limits.checkpoint_every_n_entries,
                "checkpoint_every_bytes": limits.checkpoint_every_bytes,
                "max_entry_bytes": limits.max_entry_bytes,
                "max_entry_buffer_bytes": limits.max_entry_buffer_bytes,
            },
            "notes": detected.notes,
        })
        .to_string(),
        tool_version: env!("CARGO_PKG_VERSION").into(),
    })?;

    let _ = matter.append_audit(AuditEventInput {
        actor: "system".into(),
        action: "ingest.source".into(),
        entity: format!("source:{}", source.id),
        params_json: json!({
            "path": package_root,
            "kind": detected.kind.as_str(),
        })
        .to_string(),
        tool_version: env!("CARGO_PKG_VERSION").into(),
    })?;

    let mut session = ExpandSession {
        matter,
        source_id: &source.id,
        job_id,
        limits,
        cancel,
        cursor: ExpandCursor::new(&source.id, &package_root),
        since_cp_entries: 0,
        since_cp_bytes: 0,
        cas_puts: 0,
    };

    let run = expand_package(&mut session, path);
    finish_run(
        matter,
        &source.id,
        job_id,
        detected.kind,
        session.cursor,
        run,
    )
}

fn is_7z_package(path: &Utf8Path, notes: &[String]) -> bool {
    path.file_name()
        .map(|n| n.to_ascii_lowercase().ends_with(".7z"))
        .unwrap_or(false)
        || notes.iter().any(|n| n.to_ascii_lowercase().contains("7z"))
}

/// Ensure `job_id` exists and is in (or transitioned to) Running.
fn ensure_job_running(matter: &Matter, job_id: &str) -> Result<()> {
    let job = matter
        .get_job(job_id)
        .map_err(|_| Error::JobNotFound(job_id.to_string()))?;
    match job.state {
        JobState::Running => Ok(()),
        JobState::Pending | JobState::Paused | JobState::Failed => {
            matter.set_job_state(job_id, JobState::Running, None)?;
            Ok(())
        }
        JobState::Cancelled => {
            matter.set_job_state(job_id, JobState::Pending, None)?;
            matter.set_job_state(job_id, JobState::Running, None)?;
            Ok(())
        }
        JobState::Succeeded => Err(Error::Other(format!(
            "{job_id}: cannot run ingest on succeeded job"
        ))),
    }
}

/// Register a single-file `.7z` (or similar) as an unsupported container with full audit.
fn ingest_unsupported_7z_on_job(
    matter: &Matter,
    path: &Utf8Path,
    notes: &[String],
    job_id: &str,
) -> Result<IngestSummary> {
    use matter_core::ItemErrorInput;

    let package_root = path.as_str().to_string();
    let source = matter.insert_source(&package_root, "unsupported", "failed", None)?;

    let _ = matter.append_audit(AuditEventInput {
        actor: "system".into(),
        action: "ingest.start".into(),
        entity: format!("source:{}", source.id),
        params_json: json!({
            "path": package_root,
            "kind": "unsupported",
            "job_id": job_id,
            "notes": notes,
            "unsupported_container": "7z",
        })
        .to_string(),
        tool_version: env!("CARGO_PKG_VERSION").into(),
    })?;

    let _ = matter.append_audit(AuditEventInput {
        actor: "system".into(),
        action: "ingest.source".into(),
        entity: format!("source:{}", source.id),
        params_json: json!({
            "path": package_root,
            "kind": "unsupported",
        })
        .to_string(),
        tool_version: env!("CARGO_PKG_VERSION").into(),
    })?;

    let name = path.file_name().unwrap_or(path.as_str());
    let _ = matter.record_item_error(ItemErrorInput {
        item_id: None,
        source_id: Some(source.id.clone()),
        job_id: Some(job_id.to_string()),
        stage: "expand".into(),
        code: crate::error::codes::UNSUPPORTED_7Z.into(),
        message: format!("7z container not expanded in 0016: {name}"),
        detail: Some(notes.join("; ")),
    })?;

    let msg = format!("unsupported_7z: {package_root}");
    matter.set_job_state(job_id, JobState::Failed, Some(&msg))?;
    let _ = matter.append_audit(AuditEventInput {
        actor: "system".into(),
        action: "ingest.fail".into(),
        entity: format!("source:{}", source.id),
        params_json: json!({
            "job_id": job_id,
            "code": crate::error::codes::UNSUPPORTED_7Z,
            "message": msg,
        })
        .to_string(),
        tool_version: env!("CARGO_PKG_VERSION").into(),
    })?;

    Ok(IngestSummary {
        source_id: source.id,
        job_id: job_id.to_string(),
        kind: PackageKind::Unsupported,
        entries_ok: 0,
        entries_err: 1,
        entries_skipped: 0,
        bytes_cas: 0,
        psts_found: 0,
        nested_zips: 0,
        completed: false,
        cancelled: false,
    })
}

/// Resume a previously paused/failed ingest job from leaf checkpoints.
///
/// Same blocking-thread contract as [`ingest_path`].
pub fn resume_ingest(
    matter: &Matter,
    source_id: &str,
    job_id: &str,
    limits: &ExpandLimits,
    cancel: Option<&dyn Fn() -> bool>,
) -> Result<IngestSummary> {
    let source = matter
        .get_source(source_id)
        .map_err(|_| Error::SourceNotFound(source_id.to_string()))?;
    let job = matter
        .get_job(job_id)
        .map_err(|_| Error::JobNotFound(job_id.to_string()))?;

    let kind = parse_kind(&source.kind);
    let package = Utf8Path::new(&source.path);
    if !package.as_std_path().exists() {
        return Err(Error::PackageNotFound(source.path.clone()));
    }

    // Load expand checkpoint (source of truth).
    let mut cursor = if let Some(cp) = matter.get_checkpoint(job_id, "expand")? {
        match serde_json::from_str(&cp.cursor_json) {
            Ok(c) => c,
            Err(_) => ExpandCursor::new(source_id, &source.path),
        }
    } else if let Some(ref cj) = source.cursor_json {
        match serde_json::from_str(cj) {
            Ok(c) => c,
            Err(_) => ExpandCursor::new(source_id, &source.path),
        }
    } else {
        ExpandCursor::new(source_id, &source.path)
    };
    cursor.source_id = source_id.to_string();
    cursor.package_root = source.path.clone();

    // Transition job to running.
    match job.state {
        JobState::Running => {}
        JobState::Paused | JobState::Failed | JobState::Pending => {
            matter.set_job_state(job_id, JobState::Running, None)?;
        }
        JobState::Succeeded => {
            // Already done — return summary from inventory.
            let items = matter.list_items_for_source(source_id)?;
            return Ok(IngestSummary {
                source_id: source_id.to_string(),
                job_id: job_id.to_string(),
                kind,
                entries_ok: items.len() as u64,
                entries_err: cursor.entries_err,
                entries_skipped: cursor.entries_skipped,
                bytes_cas: cursor.bytes_extracted,
                psts_found: cursor.psts_found,
                nested_zips: cursor.nested_zips,
                completed: true,
                cancelled: false,
            });
        }
        JobState::Cancelled => {
            matter.set_job_state(job_id, JobState::Pending, None)?;
            matter.set_job_state(job_id, JobState::Running, None)?;
        }
    }

    matter.update_source(source_id, "importing", source.cursor_json.as_deref())?;

    let _ = matter.append_audit(AuditEventInput {
        actor: "system".into(),
        action: "ingest.start".into(),
        entity: format!("source:{source_id}"),
        params_json: json!({
            "path": source.path,
            "kind": kind.as_str(),
            "job_id": job_id,
            "resume": true,
            "completed_count": cursor.completed_count,
        })
        .to_string(),
        tool_version: env!("CARGO_PKG_VERSION").into(),
    })?;

    let mut session = ExpandSession {
        matter,
        source_id,
        job_id,
        limits,
        cancel,
        cursor,
        since_cp_entries: 0,
        since_cp_bytes: 0,
        cas_puts: 0,
    };

    let run = expand_package(&mut session, package);
    finish_run(matter, source_id, job_id, kind, session.cursor, run)
}

fn finish_run(
    matter: &Matter,
    source_id: &str,
    job_id: &str,
    kind: PackageKind,
    cursor: ExpandCursor,
    run: Result<()>,
) -> Result<IngestSummary> {
    let cursor_json = serde_json::to_string(&cursor)?;
    let _ = matter.put_checkpoint(
        job_id,
        "expand",
        &cursor_json,
        cursor.completed_count as i64,
    );

    match run {
        Ok(()) => {
            matter.update_source(source_id, "ready", Some(&cursor_json))?;
            matter.set_job_state(job_id, JobState::Succeeded, None)?;
            let summary = IngestSummary {
                source_id: source_id.to_string(),
                job_id: job_id.to_string(),
                kind,
                entries_ok: cursor.completed_count,
                entries_err: cursor.entries_err,
                entries_skipped: cursor.entries_skipped,
                bytes_cas: cursor.bytes_extracted,
                psts_found: cursor.psts_found,
                nested_zips: cursor.nested_zips,
                completed: true,
                cancelled: false,
            };
            let _ = matter.append_audit(AuditEventInput {
                actor: "system".into(),
                action: "ingest.complete".into(),
                entity: format!("source:{source_id}"),
                params_json: json!({
                    "job_id": job_id,
                    "entries_ok": summary.entries_ok,
                    "entries_err": summary.entries_err,
                    "entries_skipped": summary.entries_skipped,
                    "bytes_cas": summary.bytes_cas,
                    "psts_found": summary.psts_found,
                    "nested_zips": summary.nested_zips,
                })
                .to_string(),
                tool_version: env!("CARGO_PKG_VERSION").into(),
            })?;
            Ok(summary)
        }
        Err(Error::Cancelled) => {
            matter.update_source(source_id, "paused", Some(&cursor_json))?;
            matter.set_job_state(job_id, JobState::Paused, Some("cancelled by caller"))?;
            let summary = IngestSummary {
                source_id: source_id.to_string(),
                job_id: job_id.to_string(),
                kind,
                entries_ok: cursor.completed_count,
                entries_err: cursor.entries_err,
                entries_skipped: cursor.entries_skipped,
                bytes_cas: cursor.bytes_extracted,
                psts_found: cursor.psts_found,
                nested_zips: cursor.nested_zips,
                completed: false,
                cancelled: true,
            };
            let _ = matter.append_audit(AuditEventInput {
                actor: "system".into(),
                action: "ingest.fail".into(),
                entity: format!("source:{source_id}"),
                params_json: json!({
                    "job_id": job_id,
                    "code": "cancelled",
                    "message": "cancelled by caller",
                    "entries_ok": summary.entries_ok,
                    "partial": true,
                })
                .to_string(),
                tool_version: env!("CARGO_PKG_VERSION").into(),
            })?;
            Ok(summary)
        }
        Err(e) => {
            let msg = e.to_string();
            let code = e.code();
            matter.update_source(source_id, "failed", Some(&cursor_json))?;
            matter.set_job_state(job_id, JobState::Failed, Some(&msg))?;
            let _ = matter.append_audit(AuditEventInput {
                actor: "system".into(),
                action: "ingest.fail".into(),
                entity: format!("source:{source_id}"),
                params_json: json!({
                    "job_id": job_id,
                    "code": code,
                    "message": msg,
                    "entries_ok": cursor.completed_count,
                    "entries_err": cursor.entries_err,
                    "partial": cursor.completed_count > 0,
                })
                .to_string(),
                tool_version: env!("CARGO_PKG_VERSION").into(),
            })?;
            Err(e)
        }
    }
}

fn parse_kind(s: &str) -> PackageKind {
    match s {
        "single_pst" => PackageKind::SinglePst,
        "single_zip" => PackageKind::SingleZip,
        "purview_package" => PackageKind::PurviewPackage,
        "raw_dump" => PackageKind::RawDump,
        _ => PackageKind::Unsupported,
    }
}
