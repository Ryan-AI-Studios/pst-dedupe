//! Create / open matter helpers (short, non-blocking for empty open; errors to UI).

use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;

use camino::{Utf8Path, Utf8PathBuf};
use matter_core::{
    default_matter_report_dir, export_matter_report, load_case_overview, CaseOverview, Matter,
    MatterReportParams, MatterReportResult, OverviewOptions,
};

use crate::params::validate_matter_name;

/// Create a new matter under `parent / name`.
pub fn create_matter(parent: &Utf8Path, name: &str) -> Result<Utf8PathBuf, String> {
    let name = validate_matter_name(name)?;
    let root = parent.join(name);
    Matter::create(&root, name).map_err(|e| e.to_string())?;
    Ok(root)
}

/// Open an existing matter root; returns matter display name on success.
///
/// When `cleanup_temp` is true, uses [`Matter::open`] (wipes orphaned
/// `workspace/temp/`). Only safe when **no** process-runner job is writing.
/// When false, uses [`Matter::open_for_read`] (no temp wipe).
pub fn open_matter(root: &Utf8Path, cleanup_temp: bool) -> Result<String, String> {
    let matter = if cleanup_temp {
        Matter::open(root).map_err(|e| e.to_string())?
    } else {
        Matter::open_for_read(root).map_err(|e| e.to_string())?
    };
    let info = matter.info().map_err(|e| e.to_string())?;
    Ok(info.name)
}

/// Read-only refresh snapshot for the workspace panels.
#[derive(Debug, Clone, Default)]
pub struct MatterSnapshot {
    pub matter_name: String,
    pub matter_id: String,
    pub sources: Vec<SourceRow>,
    pub psts: Vec<PstRow>,
    pub jobs: Vec<JobRow>,
    pub item_count: u64,
    pub journal_mode: String,
    /// Items with `dedup_role = unique` (0 if never run).
    pub dedup_unique: u64,
    /// Items with `dedup_role = duplicate`.
    pub dedup_duplicate: u64,
    /// Matter-saved user cull presets (`cull_presets` table).
    pub cull_presets: Vec<CullPresetRow>,
    /// User processing profiles (built-ins are code constants; not listed here).
    pub processing_profiles: Vec<ProcessingProfileRow>,
    /// Workflows from `list_workflows` (built-ins ∪ user) for desk dropdown + descriptions.
    pub workflows: Vec<WorkflowRow>,
}

/// Compact cull preset row for the desk dropdown (id + display name).
#[derive(Debug, Clone)]
pub struct CullPresetRow {
    pub id: String,
    pub name: String,
}

/// Compact processing profile row for the desk dropdown.
#[derive(Debug, Clone)]
pub struct ProcessingProfileRow {
    pub id: String,
    pub name: String,
    pub is_builtin: bool,
}

/// Compact workflow row for the desk dropdown (built-in or user).
#[derive(Debug, Clone)]
pub struct WorkflowRow {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub is_builtin: bool,
}

#[derive(Debug, Clone)]
pub struct SourceRow {
    pub id: String,
    pub path: String,
    pub kind: String,
    pub status: String,
}

#[derive(Debug, Clone)]
pub struct PstRow {
    pub item_id: String,
    pub source_id: String,
    pub path: String,
    pub status: String,
    pub size_bytes: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct JobRow {
    pub id: String,
    pub kind: String,
    pub state: String,
    pub error_summary: Option<String>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    /// Parent orchestration job (`workflow_run` / `profile_run`), if any.
    pub parent_job_id: Option<String>,
}

/// Load lists via [`Matter::open_for_read`] (WAL-safe; no workspace/temp wipe).
pub fn refresh_snapshot(matter_root: &Utf8Path) -> Result<MatterSnapshot, String> {
    let matter = Matter::open_for_read(matter_root).map_err(|e| e.to_string())?;
    let info = matter.info().map_err(|e| e.to_string())?;

    let journal_mode: String = matter
        .connection()
        .query_row("PRAGMA journal_mode", [], |row| row.get::<_, String>(0))
        .unwrap_or_else(|_| "unknown".into());

    let sources = matter
        .list_sources()
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|s| SourceRow {
            id: s.id,
            path: s.path,
            kind: s.kind,
            status: s.status,
        })
        .collect();

    let psts = matter
        .list_items_by_file_category("pst")
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|i| PstRow {
            item_id: i.id,
            source_id: i.source_id.unwrap_or_default(),
            path: i.path.unwrap_or_else(|| "(no path)".into()),
            status: i.status,
            size_bytes: i.size_bytes,
        })
        .collect();

    let jobs = matter
        .list_jobs()
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|j| JobRow {
            id: j.id,
            kind: j.kind,
            state: j.state.as_str().to_string(),
            error_summary: j.error_summary,
            started_at: j.started_at,
            finished_at: j.finished_at,
            parent_job_id: j.parent_job_id,
        })
        .collect();

    let item_count = matter.count_items().map_err(|e| e.to_string())?;
    let dedup_counts = matter.count_by_dedup_role().map_err(|e| e.to_string())?;
    let cull_presets = matter
        .list_cull_presets()
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|p| CullPresetRow {
            id: p.id,
            name: p.name,
        })
        .collect();

    let processing_profiles = matter
        .list_processing_profiles()
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|p| ProcessingProfileRow {
            id: p.id,
            name: p.name,
            is_builtin: p.is_builtin,
        })
        .collect();

    let workflows = matter
        .list_workflows()
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|w| WorkflowRow {
            id: w.id,
            name: w.name,
            description: w.description,
            is_builtin: w.is_builtin,
        })
        .collect();

    Ok(MatterSnapshot {
        matter_name: info.name,
        matter_id: info.id,
        sources,
        psts,
        jobs,
        item_count,
        journal_mode,
        dedup_unique: dedup_counts.unique,
        dedup_duplicate: dedup_counts.duplicate,
        cull_presets,
        processing_profiles,
        workflows,
    })
}

// ---------------------------------------------------------------------------
// Case overview (track 0038) — always off UI thread
// ---------------------------------------------------------------------------

/// Result of a background overview load.
#[derive(Debug)]
pub enum OverviewLoadResult {
    Ok(Box<CaseOverview>),
    Err(String),
}

/// At most one in-flight overview load (background SQL / fan-out).
///
/// Concurrent refresh requests while busy set a coalesce flag; when the in-flight
/// load completes, a pending request is re-spawned so job-completion refreshes
/// are never silently dropped.
#[derive(Default)]
pub struct OverviewLoadState {
    busy: bool,
    /// True when a refresh was requested while a load was already in flight.
    pending: bool,
    /// Last root used for spawn (needed to re-issue a pending load).
    last_root: Option<Utf8PathBuf>,
    rx: Option<Receiver<OverviewLoadResult>>,
}

impl OverviewLoadState {
    pub fn is_busy(&self) -> bool {
        self.busy
    }

    /// Whether a refresh is queued to run after the current load finishes.
    #[cfg(test)]
    pub fn is_pending(&self) -> bool {
        self.pending
    }

    /// Spawn `load_case_overview` on a worker thread (never on the egui thread).
    ///
    /// If a load is already in flight, marks `pending` so a follow-up load runs
    /// after the current one completes (coalesced: multiple requests → one follow-up).
    pub fn spawn(&mut self, matter_root: Utf8PathBuf) {
        self.last_root = Some(matter_root.clone());
        if self.busy {
            self.pending = true;
            return;
        }
        self.start_load(matter_root);
    }

    fn start_load(&mut self, matter_root: Utf8PathBuf) {
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        self.busy = true;
        let _ = thread::Builder::new()
            .name("desk-overview".into())
            .spawn(move || {
                let result = match load_case_overview(&matter_root, &OverviewOptions::default()) {
                    Ok(ov) => OverviewLoadResult::Ok(Box::new(ov)),
                    Err(e) => OverviewLoadResult::Err(e.to_string()),
                };
                let _ = tx.send(result);
            });
    }

    /// Poll for a completed load. On completion, if a refresh was requested while
    /// busy, immediately spawns another load (coalesce).
    pub fn try_take(&mut self) -> Option<OverviewLoadResult> {
        let rx = self.rx.as_ref()?;
        match rx.try_recv() {
            Ok(r) => {
                self.busy = false;
                self.rx = None;
                self.reschedule_if_pending();
                Some(r)
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.busy = false;
                self.rx = None;
                self.reschedule_if_pending();
                Some(OverviewLoadResult::Err(
                    "Overview load thread ended unexpectedly.".into(),
                ))
            }
        }
    }

    fn reschedule_if_pending(&mut self) {
        if !self.pending {
            return;
        }
        self.pending = false;
        if let Some(root) = self.last_root.clone() {
            self.start_load(root);
        }
    }

    /// Clear in-flight load when switching matters.
    pub fn clear(&mut self) {
        self.busy = false;
        self.pending = false;
        self.last_root = None;
        self.rx = None;
    }
}

/// Display label for empty category buckets.
pub fn overview_category_label(raw: &str) -> &str {
    if raw.is_empty() {
        "(uncategorized)"
    } else {
        raw
    }
}

/// Display label for empty custodian buckets.
pub fn overview_custodian_label(raw: &str) -> &str {
    if raw.is_empty() {
        "(none)"
    } else {
        raw
    }
}

/// Format byte counts for KPI cards (KiB / MiB / GiB when large).
pub fn format_bytes(n: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * 1024;
    const GIB: u64 = 1024 * 1024 * 1024;
    if n >= GIB {
        format!("{:.2} GiB", n as f64 / GIB as f64)
    } else if n >= MIB {
        format!("{:.2} MiB", n as f64 / MIB as f64)
    } else if n >= KIB {
        format!("{:.1} KiB", n as f64 / KIB as f64)
    } else {
        format!("{n} B")
    }
}

// ---------------------------------------------------------------------------
// Matter report export (track 0039) — blocking helpers for background workers
// ---------------------------------------------------------------------------

/// Default stamped report directory under `matter_root/exports/reports/`.
pub fn default_matter_report_output_dir(matter_root: &Utf8Path) -> Utf8PathBuf {
    default_matter_report_dir(matter_root)
}

/// Export matter report pack on a background thread (never call from egui).
///
/// PDF is deferred (D-0039-01); always `include_pdf: false`.
pub fn export_matter_report_blocking(
    matter_root: &Utf8Path,
    output_dir: &Utf8Path,
) -> Result<MatterReportResult, String> {
    export_matter_report(
        matter_root,
        MatterReportParams {
            output_dir: output_dir.to_path_buf(),
            overview_opts: OverviewOptions::default(),
            include_pdf: false,
            export_all_jobs: true,
        },
    )
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn utf8_temp() -> (TempDir, Utf8PathBuf) {
        let tmp = TempDir::new().unwrap();
        let p = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).expect("utf8 temp");
        (tmp, p)
    }

    #[test]
    fn create_open_refresh_and_wal() {
        let (_t, base) = utf8_temp();
        let root = create_matter(&base, "SmokeCase").expect("create");
        let name = open_matter(&root, true).expect("open");
        assert_eq!(name, "SmokeCase");
        let name_ro = open_matter(&root, false).expect("open_for_read");
        assert_eq!(name_ro, "SmokeCase");

        let snap = refresh_snapshot(&root).expect("snap");
        assert_eq!(snap.matter_name, "SmokeCase");
        assert!(snap.sources.is_empty());
        assert_eq!(snap.item_count, 0);
        assert_eq!(snap.journal_mode.to_lowercase(), "wal");
        assert!(
            snap.cull_presets.is_empty(),
            "fresh matter has no user cull presets"
        );
        // Built-ins appear via list_processing_profiles even on a fresh matter.
        assert!(
            snap.processing_profiles
                .iter()
                .any(|p| p.is_builtin && p.name == "standard"),
            "snapshot should include built-in processing profiles"
        );
        // Built-ins appear via list_workflows (built-ins ∪ user) on a fresh matter.
        assert!(
            snap.workflows
                .iter()
                .any(|w| w.is_builtin && w.name == "reduce_only_chain"),
            "snapshot should include built-in workflows"
        );
    }

    #[test]
    fn refresh_includes_user_cull_presets() {
        let (_t, base) = utf8_temp();
        let root = create_matter(&base, "CullPresetCase").expect("create");

        let matter = Matter::open(&root).expect("open");
        matter
            .upsert_cull_preset(matter_core::CullPresetInput {
                id: None,
                name: "my_rules".into(),
                description: Some("desk smoke".into()),
                rules_json: r#"[{"type":"dedup_unique"}]"#.into(),
                created_by: None,
            })
            .expect("upsert");
        drop(matter);

        let snap = refresh_snapshot(&root).expect("snap");
        assert_eq!(snap.cull_presets.len(), 1);
        assert_eq!(snap.cull_presets[0].name, "my_rules");
        assert!(!snap.cull_presets[0].id.is_empty());
    }

    #[test]
    fn bad_name_rejected() {
        let (_t, base) = utf8_temp();
        assert!(create_matter(&base, "").is_err());
        assert!(create_matter(&base, "a/b").is_err());
    }

    #[test]
    fn overview_label_helpers() {
        assert_eq!(overview_category_label(""), "(uncategorized)");
        assert_eq!(overview_category_label("email"), "email");
        assert_eq!(overview_custodian_label(""), "(none)");
        assert_eq!(overview_custodian_label("Alice"), "Alice");
        assert_eq!(format_bytes(500), "500 B");
        assert!(format_bytes(5_000_000).contains("MiB") || format_bytes(5_000_000).contains("KiB"));
    }

    #[test]
    fn export_matter_report_blocking_writes_pack() {
        let (_t, base) = utf8_temp();
        let root = create_matter(&base, "ReportExport").expect("create");
        let out = base.join("report_out");
        let result = export_matter_report_blocking(&root, &out).expect("export");
        assert!(!result.pdf_written);
        assert!(out.join("summary.csv").exists());
        assert!(out.join("jobs.csv").exists());
        assert!(out.join("errors_by_code.csv").exists());
        let default = default_matter_report_output_dir(&root);
        assert!(default.as_str().contains("exports"));
        assert!(default.as_str().contains("reports"));
    }

    #[test]
    fn overview_load_off_ui_thread_completes() {
        let (_t, base) = utf8_temp();
        let root = create_matter(&base, "OverviewLoad").expect("create");
        let mut state = OverviewLoadState::default();
        state.spawn(root);
        assert!(state.is_busy());
        let mut got = None;
        for _ in 0..200 {
            if let Some(r) = state.try_take() {
                got = Some(r);
                break;
            }
            thread::sleep(std::time::Duration::from_millis(10));
        }
        match got.expect("overview result") {
            OverviewLoadResult::Ok(ov) => {
                assert_eq!(ov.totals.items_total, 0);
                let _ = ov;
            }
            OverviewLoadResult::Err(e) => panic!("overview failed: {e}"),
        }
        assert!(!state.is_busy());
        assert!(!state.is_pending());
    }

    /// Double refresh while busy must coalesce (pending) and re-spawn after take.
    #[test]
    fn overview_load_double_request_coalesces_pending() {
        let (_t, base) = utf8_temp();
        let root = create_matter(&base, "OverviewCoalesce").expect("create");
        let mut state = OverviewLoadState::default();
        state.spawn(root.clone());
        assert!(state.is_busy());
        assert!(!state.is_pending());

        // Second request while busy → pending flag, not dropped.
        state.spawn(root.clone());
        assert!(state.is_busy());
        assert!(state.is_pending());
        // Further requests stay coalesced to a single follow-up.
        state.spawn(root);
        assert!(state.is_pending());

        // First completion delivers a result and immediately re-spawns.
        let mut first = None;
        for _ in 0..200 {
            if let Some(r) = state.try_take() {
                first = Some(r);
                break;
            }
            thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(matches!(
            first.expect("first overview result"),
            OverviewLoadResult::Ok(_)
        ));
        assert!(
            state.is_busy(),
            "pending refresh must re-spawn after first completion"
        );
        assert!(!state.is_pending());

        // Second load completes and leaves idle.
        let mut second = None;
        for _ in 0..200 {
            if let Some(r) = state.try_take() {
                second = Some(r);
                break;
            }
            thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(matches!(
            second.expect("second overview result"),
            OverviewLoadResult::Ok(_)
        ));
        assert!(!state.is_busy());
        assert!(!state.is_pending());
    }

    /// Concurrent reader during a held writer connection (WAL / open_for_read).
    #[test]
    fn open_for_read_while_writer_connected() {
        use std::sync::{Arc, Barrier};
        use std::thread;

        let (_t, base) = utf8_temp();
        let root = create_matter(&base, "Concurrent").expect("create");
        let barrier = Arc::new(Barrier::new(2));

        let writer_root = root.clone();
        let b_w = Arc::clone(&barrier);
        let writer = thread::spawn(move || {
            let matter = Matter::open(&writer_root).expect("writer open");
            // Hold connection open; insert a source while reader runs.
            b_w.wait();
            let _ = matter
                .insert_source(r"C:\exports\pkg", "folder", "importing", None)
                .expect("insert");
            // Keep alive briefly so reader overlaps.
            thread::sleep(std::time::Duration::from_millis(50));
            drop(matter);
        });

        let reader_root = root.clone();
        let b_r = Arc::clone(&barrier);
        let reader = thread::spawn(move || {
            b_r.wait();
            // May see empty or one source depending on race; must not hard-fail.
            let mut ok = false;
            for _ in 0..20 {
                match refresh_snapshot(&reader_root) {
                    Ok(snap) => {
                        assert_eq!(snap.matter_name, "Concurrent");
                        assert_eq!(snap.journal_mode.to_lowercase(), "wal");
                        ok = true;
                        break;
                    }
                    Err(e) => {
                        assert!(
                            crate::params::is_transient_sqlite_lock(&e),
                            "unexpected refresh error: {e}"
                        );
                        thread::sleep(std::time::Duration::from_millis(10));
                    }
                }
            }
            assert!(ok, "open_for_read refresh never succeeded under writer");
        });

        writer.join().expect("writer");
        reader.join().expect("reader");
    }
}
