//! Top-level Dedupe Desk application state.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use camino::{Utf8Path, Utf8PathBuf};
use eframe::egui;
use matter_core::CaseOverview;
use process_runner::{
    ExtractPstHandler, IngestHandler, JobParams, MatterClassifyHandler, MatterCullHandler,
    MatterDedupeHandler, MatterFtsIndexHandler, MatterIcsExtractHandler, MatterNearDupHandler,
    MatterOcrHandler, MatterOfficeExtractHandler, MatterPdfExtractHandler, MatterPromoteHandler,
    MatterThreadHandler, ProcessRunner, RunnerConfig,
};
use tokio::sync::watch;

use crate::dialogs::{DialogKind, DialogState};
use crate::matter_ops::{MatterOpResult, MatterOpState};
use crate::matter_ui::{self, MatterSnapshot, OverviewLoadResult, OverviewLoadState};
use crate::nav::{self, Screen};
use crate::params::{self, format_runner_error, is_transient_sqlite_lock};
use crate::progress_ui;
use crate::review_ui::{self, ReviewState};
use crate::settings::DeskSettings;
use crate::workspace;

/// Pending extract targets for sequential queue (single-flight runner).
#[derive(Debug, Clone)]
struct ExtractTarget {
    source_id: String,
    pst_item_id: String,
}

/// Main egui application.
pub struct DeskApp {
    screen: Screen,
    pub(crate) runner: ProcessRunner,
    pub(crate) progress_rx: watch::Receiver<process_runner::JobProgressSnapshot>,
    pub(crate) matter_root: Option<Utf8PathBuf>,
    pub(crate) matter_name: Option<String>,
    pub(crate) snapshot: MatterSnapshot,
    /// Case overview (track 0038); loaded only on a background thread.
    pub(crate) case_overview: Option<CaseOverview>,
    pub(crate) overview_loading: bool,
    overview_load: OverviewLoadState,
    /// Matter report export (track 0039) — background worker state.
    pub(crate) report_export_busy: bool,
    pub(crate) report_export_status: Option<String>,
    pub(crate) report_export_error: Option<String>,
    report_export_rx: Option<Receiver<Result<String, String>>>,
    pub(crate) selected_pst: Option<String>,
    pub(crate) dialog: DialogState,
    matter_op: MatterOpState,
    settings: DeskSettings,
    create_name: String,
    error_msg: Option<String>,
    status_msg: Option<String>,
    about_open: bool,
    /// Sequential extract queue.
    extract_queue: VecDeque<ExtractTarget>,
    last_progress_state: String,
    last_refresh: Instant,
    /// Job id of the last known active job (for resume).
    last_job_id: Option<String>,
    /// Selected cull preset for the workspace dropdown.
    /// Built-ins: bare name (`unique_only`). User presets: `user:<id>`.
    pub(crate) cull_preset: String,
    /// Selected promote policy for the workspace dropdown (`auto` + named).
    pub(crate) promote_policy: String,
    /// Review screen state (thin list + body loader).
    pub(crate) review: ReviewState,
}

impl DeskApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let mut runner = ProcessRunner::new(RunnerConfig::default());
        runner.register(Arc::new(IngestHandler::new()));
        runner.register(Arc::new(ExtractPstHandler::new()));
        runner.register(Arc::new(MatterDedupeHandler::new()));
        runner.register(Arc::new(MatterThreadHandler::new()));
        runner.register(Arc::new(MatterNearDupHandler::new()));
        runner.register(Arc::new(MatterCullHandler::new()));
        runner.register(Arc::new(MatterPromoteHandler::new()));
        runner.register(Arc::new(MatterFtsIndexHandler::new()));
        runner.register(Arc::new(MatterOfficeExtractHandler::new()));
        runner.register(Arc::new(MatterPdfExtractHandler::new()));
        runner.register(Arc::new(MatterIcsExtractHandler::new()));
        runner.register(Arc::new(MatterOcrHandler::new()));
        runner.register(Arc::new(MatterClassifyHandler::new()));
        let progress_rx = runner.watch_progress();
        let settings = DeskSettings::load();

        Self {
            screen: Screen::Home,
            runner,
            progress_rx,
            matter_root: None,
            matter_name: None,
            snapshot: MatterSnapshot::default(),
            case_overview: None,
            overview_loading: false,
            overview_load: OverviewLoadState::default(),
            report_export_busy: false,
            report_export_status: None,
            report_export_error: None,
            report_export_rx: None,
            selected_pst: None,
            dialog: DialogState::default(),
            matter_op: MatterOpState::default(),
            settings,
            create_name: String::new(),
            error_msg: None,
            status_msg: None,
            about_open: false,
            extract_queue: VecDeque::new(),
            last_progress_state: "idle".into(),
            last_refresh: Instant::now() - Duration::from_secs(60),
            last_job_id: None,
            cull_preset: "unique_only".into(),
            promote_policy: "auto".into(),
            review: ReviewState::default(),
        }
    }

    pub(crate) fn runner_busy(&self) -> bool {
        self.runner.is_busy()
            || self.progress_rx.borrow().state == "running"
            || !self.extract_queue.is_empty()
    }

    /// True when opening/switching matters with temp cleanup would be unsafe.
    fn job_may_be_writing(&self) -> bool {
        self.runner.is_busy() || self.progress_rx.borrow().state == "running"
    }

    pub(crate) fn spawn_add_folder(&mut self) {
        self.dialog.spawn(DialogKind::AddSourceFolder, None);
    }

    pub(crate) fn spawn_add_zip(&mut self) {
        self.dialog.spawn(DialogKind::AddZipFile, None);
    }

    pub(crate) fn spawn_add_pst(&mut self) {
        self.dialog.spawn(DialogKind::AddPstFile, None);
    }

    pub(crate) fn refresh_matter_lists(&mut self) {
        let Some(root) = self.matter_root.clone() else {
            return;
        };
        match matter_ui::refresh_snapshot(&root) {
            Ok(snap) => {
                self.snapshot = snap;
                self.matter_name = Some(self.snapshot.matter_name.clone());
                self.last_refresh = Instant::now();
                // Keep selection if still present.
                if let Some(sel) = &self.selected_pst {
                    if !self.snapshot.psts.iter().any(|p| &p.item_id == sel) {
                        self.selected_pst = None;
                    }
                }
            }
            Err(e) => {
                // Transient SQLITE_BUSY / "database is locked": soft status, not hard fail.
                if is_transient_sqlite_lock(&e) {
                    // Soft path only — do not sleep on the UI thread; periodic
                    // refresh while Running will retry.
                    self.status_msg = Some("Matter busy; will retry refresh…".into());
                } else {
                    self.error_msg = Some(format!("Refresh failed: {e}"));
                }
            }
        }
        // Overview SQL always off UI thread (concurrent fan-out in matter-core).
        self.request_overview_refresh();
    }

    /// Kick a background case-overview load.
    ///
    /// If a load is already in flight, the request is coalesced (pending flag) so
    /// job-completion refreshes are not silently dropped.
    pub(crate) fn request_overview_refresh(&mut self) {
        let Some(root) = self.matter_root.clone() else {
            return;
        };
        self.overview_loading = true;
        self.overview_load.spawn(root);
    }

    fn poll_overview_load(&mut self) {
        if let Some(result) = self.overview_load.try_take() {
            match result {
                OverviewLoadResult::Ok(ov) => {
                    self.case_overview = Some(*ov);
                }
                OverviewLoadResult::Err(e) => {
                    if is_transient_sqlite_lock(&e) {
                        self.status_msg = Some("Overview busy; will retry…".into());
                    } else {
                        self.status_msg = Some(format!("Overview refresh failed: {e}"));
                    }
                }
            }
            // try_take may have re-spawned a coalesced pending refresh.
            self.overview_loading = self.overview_load.is_busy();
        }
    }

    fn set_matter(&mut self, root: Utf8PathBuf, name: String) {
        self.matter_root = Some(root.clone());
        self.matter_name = Some(name);
        self.settings.remember_matter(root.as_str());
        self.settings.save();
        self.screen = Screen::Workspace;
        self.extract_queue.clear();
        self.selected_pst = None;
        // Reset cull selection so a prior matter's user:<id> cannot leak.
        self.cull_preset = "unique_only".into();
        // Clear review corpus state for the previous matter.
        self.review.clear_for_matter_change();
        self.case_overview = None;
        self.overview_load.clear();
        self.overview_loading = false;
        self.report_export_busy = false;
        self.report_export_status = None;
        self.report_export_error = None;
        self.report_export_rx = None;
        self.refresh_matter_lists();
        self.status_msg = Some(format!("Opened matter at {root}"));
    }

    /// Poll background matter-report export worker.
    pub(crate) fn poll_report_export(&mut self) {
        let Some(rx) = self.report_export_rx.as_ref() else {
            return;
        };
        match rx.try_recv() {
            Ok(Ok(msg)) => {
                self.report_export_busy = false;
                self.report_export_rx = None;
                self.report_export_status = Some(msg);
                self.report_export_error = None;
            }
            Ok(Err(e)) => {
                self.report_export_busy = false;
                self.report_export_rx = None;
                self.report_export_error = Some(e);
                self.report_export_status = None;
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.report_export_busy = false;
                self.report_export_rx = None;
                self.report_export_error = Some("Report export thread ended unexpectedly.".into());
            }
        }
    }

    /// Spawn matter report CSV pack export on a background thread (rfd folder picker).
    pub(crate) fn spawn_report_export(&mut self, ctx: &egui::Context) {
        if self.report_export_busy {
            return;
        }
        let Some(root) = self.matter_root.clone() else {
            self.report_export_error = Some("No matter open.".into());
            return;
        };
        let default_dir = matter_ui::default_matter_report_output_dir(&root);
        let ctx = ctx.clone();
        let (tx, rx) = mpsc::channel();
        self.report_export_rx = Some(rx);
        self.report_export_busy = true;
        self.report_export_status = Some("Writing report…".into());
        self.report_export_error = None;
        let _ = thread::Builder::new()
            .name("desk-report-export".into())
            .spawn(move || {
                // Folder picker on background thread (never on egui).
                // Cancel → fall back to default stamped path under exports/reports/.
                let chosen = rfd::FileDialog::new()
                    .set_title("Export matter report folder")
                    .pick_folder();
                let output_dir = match chosen {
                    Some(p) => match Utf8PathBuf::from_path_buf(p) {
                        Ok(folder) => {
                            // Operator chose a parent folder; write a fresh stamp subdir
                            // so we never silently clobber an existing pack.
                            let stamp = default_dir
                                .file_name()
                                .unwrap_or("matter_report")
                                .to_string();
                            folder.join(stamp)
                        }
                        Err(_) => {
                            let _ = tx.send(Err("Export path is not valid UTF-8.".into()));
                            ctx.request_repaint();
                            return;
                        }
                    },
                    None => default_dir,
                };
                let result =
                    matter_ui::export_matter_report_blocking(&root, &output_dir).map(|r| {
                        format!(
                            "Matter report written ({} file(s), items={}) → {}",
                            r.files_written.len(),
                            r.overview.totals.items_total,
                            r.output_dir
                        )
                    });
                let _ = tx.send(result);
                ctx.request_repaint();
            });
    }

    /// Navigate to Review and force a thin-list reload.
    pub(crate) fn open_review(&mut self) {
        if self.matter_root.is_none() {
            return;
        }
        self.review.request_reload();
        self.screen = Screen::Review;
    }

    fn create_matter_at(&mut self, parent: PathBuf) {
        if self.job_may_be_writing() {
            self.error_msg = Some(
                "A job is still running. Cancel or wait before creating another matter.".into(),
            );
            return;
        }
        if self.matter_op.is_busy() {
            return;
        }
        let parent = match Utf8PathBuf::from_path_buf(parent) {
            Ok(p) => p,
            Err(_) => {
                self.error_msg = Some("Parent path is not valid UTF-8.".into());
                return;
            }
        };
        self.settings.last_parent_dir = Some(parent.to_string());
        self.settings.save();
        let name = self.create_name.clone();
        self.status_msg = Some("Creating matter…".into());
        self.matter_op.spawn_create(parent, name);
    }

    fn open_matter_at(&mut self, path: PathBuf) {
        if self.job_may_be_writing() {
            self.error_msg = Some(
                "A job is still running. Cancel or wait before opening another matter \
                 (temp cleanup must not race extract)."
                    .into(),
            );
            return;
        }
        if self.matter_op.is_busy() {
            return;
        }
        self.status_msg = Some("Opening matter…".into());
        // Off UI thread: migrations + workspace/temp cleanup.
        self.matter_op.spawn_open(path);
    }

    fn poll_matter_op(&mut self) {
        if let Some(result) = self.matter_op.try_take() {
            match result {
                MatterOpResult::Created { root, name } => {
                    self.create_name.clear();
                    self.error_msg = None;
                    self.set_matter(root, name);
                }
                MatterOpResult::Opened { root, name } => {
                    self.error_msg = None;
                    self.set_matter(root, name);
                }
                MatterOpResult::Failed { message } => {
                    self.error_msg = Some(message);
                    self.status_msg = None;
                }
            }
        }
    }

    fn start_ingest_path(&mut self, path: PathBuf) {
        let Some(root) = self.matter_root.clone() else {
            self.error_msg = Some("No matter open.".into());
            return;
        };
        let path_str = match path.to_str() {
            Some(s) => s.to_string(),
            None => {
                self.error_msg = Some("Path is not valid UTF-8.".into());
                return;
            }
        };
        let kind_hint = if params::looks_like_pst(&path_str) {
            "PST"
        } else if params::looks_like_zip(&path_str) {
            "ZIP"
        } else {
            "folder"
        };
        let params = JobParams::new(params::ingest_params(&path_str));
        match self
            .runner
            .start(Utf8Path::new(root.as_str()), "ingest", params)
        {
            Ok(job_id) => {
                self.last_job_id = Some(job_id.clone());
                self.status_msg = Some(format!("Started ingest ({kind_hint}) job {job_id}"));
                self.error_msg = None;
            }
            Err(e) => self.note_start_error(e),
        }
    }

    fn note_start_error(&mut self, e: process_runner::RunnerError) {
        if let process_runner::RunnerError::Busy { ref job_id } = e {
            self.last_job_id = Some(job_id.clone());
            self.status_msg = Some(format!(
                "Busy on job {job_id}. Click Resume to continue a leftover/active job."
            ));
        }
        self.error_msg = Some(format_runner_error(&e));
    }

    pub(crate) fn start_dedupe(&mut self) {
        let Some(root) = self.matter_root.clone() else {
            self.error_msg = Some("No matter open.".into());
            return;
        };
        let params = JobParams::new(params::dedupe_default_params());
        match self
            .runner
            .start(Utf8Path::new(root.as_str()), "dedupe", params)
        {
            Ok(job_id) => {
                self.last_job_id = Some(job_id.clone());
                self.status_msg = Some(format!("Started dedupe job {job_id}"));
                self.error_msg = None;
            }
            Err(e) => self.note_start_error(e),
        }
    }

    pub(crate) fn start_thread(&mut self) {
        let Some(root) = self.matter_root.clone() else {
            self.error_msg = Some("No matter open.".into());
            return;
        };
        let params = JobParams::new(params::thread_default_params());
        match self
            .runner
            .start(Utf8Path::new(root.as_str()), "thread", params)
        {
            Ok(job_id) => {
                self.last_job_id = Some(job_id.clone());
                self.status_msg = Some(format!("Started thread job {job_id}"));
                self.error_msg = None;
            }
            Err(e) => self.note_start_error(e),
        }
    }

    pub(crate) fn start_neardup(&mut self) {
        let Some(root) = self.matter_root.clone() else {
            self.error_msg = Some("No matter open.".into());
            return;
        };
        let params = JobParams::new(params::neardup_default_params());
        match self
            .runner
            .start(Utf8Path::new(root.as_str()), "neardup", params)
        {
            Ok(job_id) => {
                self.last_job_id = Some(job_id.clone());
                self.status_msg = Some(format!("Started near-dup job {job_id}"));
                self.error_msg = None;
            }
            Err(e) => self.note_start_error(e),
        }
    }

    pub(crate) fn start_cull(&mut self) {
        let Some(root) = self.matter_root.clone() else {
            self.error_msg = Some("No matter open.".into());
            return;
        };
        let params_json = params::cull_params_for_selection(&self.cull_preset);
        let params = JobParams::new(params_json);
        match self
            .runner
            .start(Utf8Path::new(root.as_str()), "cull", params)
        {
            Ok(job_id) => {
                self.last_job_id = Some(job_id.clone());
                let display = self.cull_preset_display_name();
                self.status_msg = Some(format!("Started cull job {job_id} (preset={display})"));
                self.error_msg = None;
            }
            Err(e) => self.note_start_error(e),
        }
    }

    pub(crate) fn start_promote(&mut self) {
        let Some(root) = self.matter_root.clone() else {
            self.error_msg = Some("No matter open.".into());
            return;
        };
        let policy = self.promote_policy.as_str();
        let params_json = params::promote_params_for_policy(policy);
        let params = JobParams::new(params_json);
        match self
            .runner
            .start(Utf8Path::new(root.as_str()), "promote", params)
        {
            Ok(job_id) => {
                self.last_job_id = Some(job_id.clone());
                self.status_msg = Some(format!(
                    "Started promote job {job_id} (policy={policy}; auto resolves at run)"
                ));
                self.error_msg = None;
            }
            Err(e) => self.note_start_error(e),
        }
    }

    /// Incremental FTS index build/update (`reset: false`).
    pub(crate) fn start_fts_index(&mut self) {
        let Some(root) = self.matter_root.clone() else {
            self.error_msg = Some("No matter open.".into());
            return;
        };
        let params = JobParams::new(params::fts_index_default_params());
        match self
            .runner
            .start(Utf8Path::new(root.as_str()), "fts_index", params)
        {
            Ok(job_id) => {
                self.last_job_id = Some(job_id.clone());
                self.status_msg = Some(format!("Started FTS index job {job_id}"));
                self.error_msg = None;
                self.review.index_outdated = false;
            }
            Err(e) => self.note_start_error(e),
        }
    }

    /// Full FTS rebuild (`reset: true`). Drops any cached reader state first.
    pub(crate) fn start_fts_rebuild(&mut self) {
        let Some(root) = self.matter_root.clone() else {
            self.error_msg = Some("No matter open.".into());
            return;
        };
        // Desk does not hold live MatterIndex Arcs; clear keyword banner state.
        self.review.keyword_error = None;
        self.review.index_outdated = false;
        let params = JobParams::new(params::fts_index_reset_params());
        match self
            .runner
            .start(Utf8Path::new(root.as_str()), "fts_index", params)
        {
            Ok(job_id) => {
                self.last_job_id = Some(job_id.clone());
                self.status_msg = Some(format!("Started FTS rebuild job {job_id} (reset:true)"));
                self.error_msg = None;
            }
            Err(e) => self.note_start_error(e),
        }
    }

    /// Extract plain text from OOXML natives (`office_extract`).
    pub(crate) fn start_office_extract(&mut self) {
        let Some(root) = self.matter_root.clone() else {
            self.error_msg = Some("No matter open.".into());
            return;
        };
        let params = JobParams::new(params::office_extract_default_params());
        match self
            .runner
            .start(Utf8Path::new(root.as_str()), "office_extract", params)
        {
            Ok(job_id) => {
                self.last_job_id = Some(job_id.clone());
                self.status_msg = Some(format!("Started office extract job {job_id}"));
                self.error_msg = None;
            }
            Err(e) => self.note_start_error(e),
        }
    }

    /// Extract embedded text from PDF natives (`pdf_extract`).
    pub(crate) fn start_pdf_extract(&mut self) {
        let Some(root) = self.matter_root.clone() else {
            self.error_msg = Some("No matter open.".into());
            return;
        };
        let params = JobParams::new(params::pdf_extract_default_params());
        match self
            .runner
            .start(Utf8Path::new(root.as_str()), "pdf_extract", params)
        {
            Ok(job_id) => {
                self.last_job_id = Some(job_id.clone());
                self.status_msg = Some(format!("Started PDF extract job {job_id}"));
                self.error_msg = None;
            }
            Err(e) => self.note_start_error(e),
        }
    }

    /// Tooltip / enablement for Workspace **Run OCR** (enable + tool paths).
    pub(crate) fn ocr_run_tooltip(&self) -> String {
        if !self.settings.ocr_enabled {
            return "Enable local OCR in Settings (Home) first — off by default".into();
        }
        if !self.ocr_tesseract_looks_available() {
            return "Tesseract not found — set Tesseract path in Settings or add to PATH".into();
        }
        "Run local Tesseract OCR on needs-OCR PDFs and images (kind=ocr). PDF pages also need pdftoppm/mutool.".into()
    }

    /// True when OCR is enabled and a Tesseract binary appears resolvable.
    pub(crate) fn ocr_run_enabled(&self) -> bool {
        self.settings.ocr_enabled && self.ocr_tesseract_looks_available()
    }

    fn ocr_tesseract_looks_available(&self) -> bool {
        if let Some(p) = self.settings.tesseract_path.as_deref().map(str::trim) {
            if !p.is_empty() {
                return std::path::Path::new(p).is_file();
            }
        }
        // Cheap PATH probe (not a full preflight — job still fails closed).
        which_on_path("tesseract") || which_on_path("tesseract.exe")
    }

    /// Run local OCR (`ocr`) — fails closed when Settings OCR is disabled.
    pub(crate) fn start_ocr(&mut self) {
        let Some(root) = self.matter_root.clone() else {
            self.error_msg = Some("No matter open.".into());
            return;
        };
        if !self.settings.ocr_enabled {
            self.error_msg =
                Some("OCR is disabled. Enable local OCR in Settings (Home) before running.".into());
            return;
        }
        if !self.ocr_tesseract_looks_available() {
            self.error_msg = Some(
                "Tesseract not found. Set the executable path in Settings or install and add to PATH."
                    .into(),
            );
            return;
        }
        let params = JobParams::new(params::ocr_default_params(
            self.settings.ocr_enabled,
            self.settings.tesseract_path.as_deref(),
            self.settings.tessdata_dir.as_deref(),
            self.settings.pdf_renderer_path.as_deref(),
        ));
        match self
            .runner
            .start(Utf8Path::new(root.as_str()), "ocr", params)
        {
            Ok(job_id) => {
                self.last_job_id = Some(job_id.clone());
                self.status_msg = Some(format!("Started OCR job {job_id}"));
                self.error_msg = None;
            }
            Err(e) => self.note_start_error(e),
        }
    }

    /// Classify file types (`classify`) using taxonomy_v1.
    pub(crate) fn start_classify(&mut self) {
        let Some(root) = self.matter_root.clone() else {
            self.error_msg = Some("No matter open.".into());
            return;
        };
        let params = JobParams::new(params::classify_default_params());
        match self
            .runner
            .start(Utf8Path::new(root.as_str()), "classify", params)
        {
            Ok(job_id) => {
                self.last_job_id = Some(job_id.clone());
                self.status_msg = Some(format!("Started classify job {job_id}"));
                self.error_msg = None;
            }
            Err(e) => self.note_start_error(e),
        }
    }

    /// Extract calendar events from ICS natives (`ics_extract`).
    pub(crate) fn start_ics_extract(&mut self) {
        let Some(root) = self.matter_root.clone() else {
            self.error_msg = Some("No matter open.".into());
            return;
        };
        let params = JobParams::new(params::ics_extract_default_params());
        match self
            .runner
            .start(Utf8Path::new(root.as_str()), "ics_extract", params)
        {
            Ok(job_id) => {
                self.last_job_id = Some(job_id.clone());
                self.status_msg = Some(format!("Started ICS extract job {job_id}"));
                self.error_msg = None;
            }
            Err(e) => self.note_start_error(e),
        }
    }

    /// Human name for the current cull selection (resolves `user:<id>` via snapshot).
    pub(crate) fn cull_preset_display_name(&self) -> String {
        if let Some(id) = self
            .cull_preset
            .strip_prefix(params::CULL_USER_PRESET_PREFIX)
        {
            self.snapshot
                .cull_presets
                .iter()
                .find(|p| p.id == id)
                .map(|p| p.name.clone())
                .unwrap_or_else(|| id.to_string())
        } else {
            self.cull_preset.clone()
        }
    }

    pub(crate) fn start_extract_selected(&mut self) {
        let Some(item_id) = self.selected_pst.clone() else {
            return;
        };
        let Some(pst) = self.snapshot.psts.iter().find(|p| p.item_id == item_id) else {
            self.error_msg = Some("Selected PST not found.".into());
            return;
        };
        if pst.source_id.is_empty() {
            self.error_msg = Some("PST row has no source_id.".into());
            return;
        }
        self.extract_queue.clear();
        self.start_extract_one(pst.source_id.clone(), pst.item_id.clone());
    }

    pub(crate) fn start_extract_all(&mut self) {
        self.extract_queue.clear();
        for pst in &self.snapshot.psts {
            if pst.source_id.is_empty() {
                continue;
            }
            self.extract_queue.push_back(ExtractTarget {
                source_id: pst.source_id.clone(),
                pst_item_id: pst.item_id.clone(),
            });
        }
        self.pump_extract_queue();
    }

    /// Start one extract. Returns whether start succeeded.
    fn start_extract_one(&mut self, source_id: String, pst_item_id: String) -> bool {
        let Some(root) = self.matter_root.clone() else {
            return false;
        };
        let params = JobParams::new(params::extract_pst_item_params(&source_id, &pst_item_id));
        match self
            .runner
            .start(Utf8Path::new(root.as_str()), "extract_pst", params)
        {
            Ok(job_id) => {
                self.last_job_id = Some(job_id.clone());
                self.status_msg = Some(format!("Started extract job {job_id}"));
                self.error_msg = None;
                true
            }
            Err(e) => {
                self.note_start_error(e);
                false
            }
        }
    }

    fn pump_extract_queue(&mut self) {
        if self.runner.is_busy() || self.progress_rx.borrow().state == "running" {
            return;
        }
        // Peek then pop only after successful start (R1-P4).
        let Some(next) = self.extract_queue.front().cloned() else {
            return;
        };
        if self.start_extract_one(next.source_id, next.pst_item_id) {
            let _ = self.extract_queue.pop_front();
        } else {
            // Unlock UI: drop queue so operator can Resume busy job then re-run Extract all.
            self.extract_queue.clear();
            self.status_msg = Some(
                "Extract queue cleared (start failed). Resume any busy job, then Extract again."
                    .into(),
            );
        }
    }

    pub(crate) fn cancel_active(&mut self) {
        let snap = self.progress_rx.borrow().clone();
        let id = if !snap.job_id.is_empty() {
            snap.job_id.clone()
        } else if let Some(id) = self.last_job_id.clone() {
            id
        } else {
            self.error_msg = Some("No job to cancel.".into());
            return;
        };
        // Drop remaining queue on cancel so we don't auto-start more.
        self.extract_queue.clear();
        match self.runner.cancel(&id) {
            Ok(()) => self.status_msg = Some(format!("Cancel requested for {id}")),
            Err(e) => self.error_msg = Some(format_runner_error(&e)),
        }
    }

    pub(crate) fn resume_active(&mut self) {
        let Some(root) = self.matter_root.clone() else {
            return;
        };
        let snap = self.progress_rx.borrow().clone();
        let id = if !snap.job_id.is_empty() {
            snap.job_id.clone()
        } else if let Some(id) = self.last_job_id.clone() {
            id
        } else if let Some(j) = self
            .snapshot
            .jobs
            .iter()
            .find(|j| j.state == "running" || j.state == "paused" || j.state == "failed")
        {
            j.id.clone()
        } else {
            self.error_msg = Some("No job to resume.".into());
            return;
        };
        match self.runner.resume(Utf8Path::new(root.as_str()), &id) {
            Ok(()) => {
                self.last_job_id = Some(id.clone());
                self.status_msg = Some(format!("Resumed job {id}"));
                self.error_msg = None;
            }
            Err(e) => self.error_msg = Some(format_runner_error(&e)),
        }
    }

    /// Whether Resume should be enabled (watch snapshot, last id, or durable job row).
    pub(crate) fn can_resume(&self) -> bool {
        let snap = self.progress_rx.borrow().clone();
        // Live in-process Running: wait for cancel/pause, do not spam Resume.
        if snap.state == "running" && self.runner.is_busy() {
            return false;
        }
        if !snap.job_id.is_empty() && matches!(snap.state.as_str(), "paused" | "failed") {
            return true;
        }
        // Durable leftover Running / paused / failed (including Busy-seeded id).
        if let Some(id) = &self.last_job_id {
            if self
                .snapshot
                .jobs
                .iter()
                .any(|j| j.id == *id && matches!(j.state.as_str(), "running" | "paused" | "failed"))
            {
                return true;
            }
            // Busy seed before jobs list has refreshed: allow one resume attempt.
            if self.snapshot.jobs.is_empty() {
                return true;
            }
        }
        self.snapshot
            .jobs
            .iter()
            .any(|j| matches!(j.state.as_str(), "running" | "paused" | "failed"))
    }

    fn poll_dialog(&mut self) {
        if let Some(result) = self.dialog.try_take() {
            match result.kind {
                DialogKind::CreateParentFolder => {
                    if let Some(path) = result.path {
                        self.create_matter_at(path);
                    }
                }
                DialogKind::OpenMatterFolder => {
                    if let Some(path) = result.path {
                        self.open_matter_at(path);
                    }
                }
                DialogKind::AddSourceFolder | DialogKind::AddZipFile | DialogKind::AddPstFile => {
                    if let Some(path) = result.path {
                        self.start_ingest_path(path);
                    }
                }
            }
        }
    }

    fn on_progress_tick(&mut self) {
        let snap = self.progress_rx.borrow().clone();
        if !snap.job_id.is_empty() {
            self.last_job_id = Some(snap.job_id.clone());
        }
        let state = snap.state.clone();
        if state != self.last_progress_state {
            let prev = self.last_progress_state.clone();
            self.last_progress_state = state.clone();
            // Refresh lists when a job ends or starts.
            if prev == "running" || state == "succeeded" || state == "paused" || state == "failed" {
                self.refresh_matter_lists();
                // After FTS (or any) job terminal state, re-load Review list so
                // keyword results / index-outdated banners pick up the new index.
                if prev == "running"
                    && (state == "succeeded" || state == "paused" || state == "failed")
                {
                    self.review.request_reload();
                    self.review.index_outdated = false;
                    if state == "succeeded" || state == "paused" {
                        self.pump_extract_queue();
                    }
                }
            }
        }
        // Periodic refresh while running (WAL concurrent read).
        if state == "running" && self.last_refresh.elapsed() > Duration::from_secs(2) {
            self.refresh_matter_lists();
        }
    }

    fn show_home(&mut self, ui: &mut egui::Ui) {
        ui.heading("Matters");
        ui.label("Create or open a matter to begin.");
        ui.add_space(8.0);

        ui.horizontal(|ui| {
            ui.label("New matter name:");
            ui.text_edit_singleline(&mut self.create_name);
            let can_create = !self.dialog.is_open()
                && !self.matter_op.is_busy()
                && !self.create_name.trim().is_empty()
                && !self.job_may_be_writing();
            if ui
                .add_enabled(can_create, egui::Button::new("Create matter…"))
                .clicked()
            {
                let initial = self.settings.last_parent_dir.as_ref().map(PathBuf::from);
                self.dialog.spawn(DialogKind::CreateParentFolder, initial);
            }
        });

        ui.add_space(6.0);
        if ui
            .add_enabled(
                !self.dialog.is_open() && !self.matter_op.is_busy() && !self.job_may_be_writing(),
                egui::Button::new("Open matter folder…"),
            )
            .clicked()
        {
            self.dialog.spawn(DialogKind::OpenMatterFolder, None);
        }

        if self.dialog.is_open() {
            ui.label("File dialog open…");
        }
        if self.matter_op.is_busy() {
            ui.label("Opening or creating matter…");
        }
        if self.job_may_be_writing() {
            ui.colored_label(
                egui::Color32::from_rgb(180, 120, 40),
                "Job running — finish or cancel before creating/opening another matter.",
            );
        }

        ui.add_space(12.0);
        ui.heading("Settings");
        ui.horizontal(|ui| {
            ui.label("Reviewer (actor):");
            let response = ui.add(
                egui::TextEdit::singleline(&mut self.settings.reviewer_name)
                    .desired_width(160.0)
                    .hint_text("desk"),
            );
            if response.changed() || response.lost_focus() {
                self.settings.save();
            }
            ui.label(
                egui::RichText::new(format!("audit as \"{}\"", self.settings.actor()))
                    .weak()
                    .small(),
            );
        });
        ui.label(
            egui::RichText::new("Used as coding audit actor on Review (empty → desk).")
                .weak()
                .small(),
        );

        ui.add_space(8.0);
        ui.heading("Local OCR (optional)");
        let ocr_resp = ui.checkbox(
            &mut self.settings.ocr_enabled,
            "Enable local OCR (Tesseract CLI)",
        );
        if ocr_resp.changed() {
            self.settings.save();
        }
        ui.label(
            egui::RichText::new(
                "Off by default. Requires system Tesseract with OSD (osd.traineddata). \
                 PDF OCR also needs pdftoppm (Poppler) or mutool (MuPDF). No cloud OCR.",
            )
            .weak()
            .small(),
        );
        ui.horizontal(|ui| {
            ui.label("Tesseract path:");
            let mut path = self.settings.tesseract_path.clone().unwrap_or_default();
            let r = ui.add(
                egui::TextEdit::singleline(&mut path)
                    .desired_width(280.0)
                    .hint_text("optional — uses PATH if empty"),
            );
            if r.changed() || r.lost_focus() {
                self.settings.tesseract_path = if path.trim().is_empty() {
                    None
                } else {
                    Some(path.trim().to_string())
                };
                self.settings.save();
            }
        });
        ui.horizontal(|ui| {
            ui.label("Tessdata dir:");
            let mut path = self.settings.tessdata_dir.clone().unwrap_or_default();
            let r = ui.add(
                egui::TextEdit::singleline(&mut path)
                    .desired_width(280.0)
                    .hint_text("optional TESSDATA_PREFIX"),
            );
            if r.changed() || r.lost_focus() {
                self.settings.tessdata_dir = if path.trim().is_empty() {
                    None
                } else {
                    Some(path.trim().to_string())
                };
                self.settings.save();
            }
        });
        ui.horizontal(|ui| {
            ui.label("PDF renderer:");
            let mut path = self.settings.pdf_renderer_path.clone().unwrap_or_default();
            let r = ui.add(
                egui::TextEdit::singleline(&mut path)
                    .desired_width(280.0)
                    .hint_text("optional pdftoppm or mutool"),
            );
            if r.changed() || r.lost_focus() {
                self.settings.pdf_renderer_path = if path.trim().is_empty() {
                    None
                } else {
                    Some(path.trim().to_string())
                };
                self.settings.save();
            }
        });

        ui.add_space(12.0);
        ui.heading("Recent");
        if self.settings.recent_matters.is_empty() {
            ui.label("No recent matters.");
        } else {
            let recent = self.settings.recent_matters.clone();
            let can_open_recent =
                !self.dialog.is_open() && !self.matter_op.is_busy() && !self.job_may_be_writing();
            for path in recent {
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(can_open_recent, egui::Button::new("Open"))
                        .clicked()
                    {
                        self.open_matter_at(PathBuf::from(&path));
                    }
                    ui.label(&path);
                });
            }
        }
    }

    fn show_stub(&mut self, ui: &mut egui::Ui, title: &str) {
        ui.heading(title);
        ui.label("Coming soon — later tracks will enable this area.");
        ui.label("Continue using Workspace for sources and process jobs.");
        if ui.button("Back to Workspace").clicked() {
            if self.matter_root.is_some() {
                self.screen = Screen::Workspace;
            } else {
                self.screen = Screen::Home;
            }
        }
    }

    fn show_nav(&mut self, ui: &mut egui::Ui) {
        let has_matter = self.matter_root.is_some();
        ui.horizontal(|ui| {
            for target in [
                Screen::Home,
                Screen::Workspace,
                Screen::StubReduce,
                Screen::Review,
                Screen::StubProduce,
            ] {
                let selected = self.screen == target;
                let enabled = target == Screen::Home || has_matter;
                let label = if target.is_stub() {
                    format!("{} (soon)", target.label())
                } else {
                    target.label().to_string()
                };
                if ui
                    .add_enabled(enabled, egui::Button::selectable(selected, label))
                    .clicked()
                {
                    let next = nav::resolve_nav(self.screen, target, has_matter);
                    if next == Screen::Review && self.screen != Screen::Review {
                        self.review.request_reload();
                    }
                    self.screen = next;
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("About").clicked() {
                    self.about_open = true;
                }
            });
        });
    }
}

impl eframe::App for DeskApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        self.poll_dialog();
        self.poll_matter_op();
        self.poll_overview_load();
        self.poll_report_export();
        self.on_progress_tick();

        let snap = self.progress_rx.borrow().clone();
        progress_ui::request_job_repaint(&ctx, &snap);
        // Also repaint lightly while a dialog, matter op, overview load, or report export is in flight.
        if self.dialog.is_open()
            || self.matter_op.is_busy()
            || self.overview_load.is_busy()
            || self.report_export_busy
        {
            ctx.request_repaint_after(Duration::from_millis(100));
        }

        egui::Panel::top("header").show_inside(ui, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.heading("Dedupe Desk");
                ui.label("— local-first eDiscovery workstation");
            });
            ui.add_space(2.0);
            self.show_nav(ui);
            ui.add_space(2.0);
        });

        if let Some(err) = self.error_msg.clone() {
            egui::Panel::top("error_banner").show_inside(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.colored_label(
                        egui::Color32::from_rgb(200, 60, 60),
                        format!("Error: {err}"),
                    );
                    if ui.button("Dismiss").clicked() {
                        self.error_msg = None;
                    }
                });
            });
        }
        if let Some(status) = self.status_msg.clone() {
            egui::Panel::top("status_banner").show_inside(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(&status);
                    if ui.button("Dismiss").clicked() {
                        self.status_msg = None;
                    }
                });
            });
        }

        egui::CentralPanel::default().show_inside(ui, |ui| match self.screen {
            Screen::Home => self.show_home(ui),
            Screen::Workspace => workspace::show(ui, self),
            Screen::StubReduce => self.show_stub(ui, "Reduce"),
            Screen::Review => {
                if let Some(root) = self.matter_root.clone() {
                    let actor = self.settings.actor().to_string();
                    let mut fts_req = None;
                    let index_job_busy = self.job_may_be_writing();
                    review_ui::show(
                        ui,
                        &mut self.review,
                        &root,
                        &actor,
                        &mut fts_req,
                        index_job_busy,
                    );
                    match fts_req {
                        Some(review_ui::FtsUiRequest::UpdateIndex) => self.start_fts_index(),
                        Some(review_ui::FtsUiRequest::RebuildIndex) => self.start_fts_rebuild(),
                        None => {}
                    }
                } else {
                    ui.label("Open a matter to review.");
                }
            }
            Screen::StubProduce => self.show_stub(ui, "Produce"),
        });

        if self.about_open {
            egui::Window::new("About Dedupe Desk")
                .collapsible(false)
                .resizable(false)
                .show(&ctx, |ui| {
                    ui.label(format!("Dedupe Desk v{}", env!("CARGO_PKG_VERSION")));
                    ui.label("Offline-first · single-exe · no servers");
                    ui.label("Process work runs on an in-process matter worker.");
                    ui.label("Legacy scan GUI: pst-dedup-gui");
                    if ui.button("Close").clicked() {
                        self.about_open = false;
                    }
                });
        }
    }

    fn on_exit(&mut self) {
        self.runner.shutdown();
    }
}

impl Drop for DeskApp {
    fn drop(&mut self) {
        // Belt-and-suspenders: join worker even if on_exit was skipped.
        self.runner.shutdown();
    }
}

fn which_on_path(name: &str) -> bool {
    let Some(path_var) = std::env::var_os("PATH") else {
        return false;
    };
    for dir in std::env::split_paths(&path_var) {
        if dir.join(name).is_file() {
            return true;
        }
    }
    false
}
