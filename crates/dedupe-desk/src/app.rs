//! Top-level Dedupe Desk application state.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use camino::{Utf8Path, Utf8PathBuf};
use eframe::egui;
use process_runner::{ExtractPstHandler, IngestHandler, JobParams, ProcessRunner, RunnerConfig};
use tokio::sync::watch;

use crate::dialogs::{DialogKind, DialogState};
use crate::matter_ops::{MatterOpResult, MatterOpState};
use crate::matter_ui::{self, MatterSnapshot};
use crate::nav::{self, Screen};
use crate::params::{self, format_runner_error, is_transient_sqlite_lock};
use crate::progress_ui;
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
}

impl DeskApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let mut runner = ProcessRunner::new(RunnerConfig::default());
        runner.register(Arc::new(IngestHandler::new()));
        runner.register(Arc::new(ExtractPstHandler::new()));
        let progress_rx = runner.watch_progress();
        let settings = DeskSettings::load();

        Self {
            screen: Screen::Home,
            runner,
            progress_rx,
            matter_root: None,
            matter_name: None,
            snapshot: MatterSnapshot::default(),
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
    }

    fn set_matter(&mut self, root: Utf8PathBuf, name: String) {
        self.matter_root = Some(root.clone());
        self.matter_name = Some(name);
        self.settings.remember_matter(root.as_str());
        self.settings.save();
        self.screen = Screen::Workspace;
        self.extract_queue.clear();
        self.selected_pst = None;
        self.refresh_matter_lists();
        self.status_msg = Some(format!("Opened matter at {root}"));
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
                if prev == "running" && (state == "succeeded" || state == "paused") {
                    self.pump_extract_queue();
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
                Screen::StubReview,
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
                    self.screen = nav::resolve_nav(self.screen, target, has_matter);
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
        self.on_progress_tick();

        let snap = self.progress_rx.borrow().clone();
        progress_ui::request_job_repaint(&ctx, &snap);
        // Also repaint lightly while a dialog or matter op is in flight.
        if self.dialog.is_open() || self.matter_op.is_busy() {
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
            Screen::StubReview => self.show_stub(ui, "Review"),
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
