//! Top-level application state machine and egui App implementation.

use eframe::egui;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::unique_wizard::UniqueWizardForm;
use crate::unique_worker::{self, UniqueOutcomeView, UniqueProgressState};
use crate::views;
use crate::worker::{self, ScanProgress, ScanResult};

/// Application states.
#[derive(Debug, Clone, PartialEq)]
pub enum AppState {
    FileSelect,
    Settings,
    Scanning,
    Results,
    /// Unique-PST wizard (0072).
    UniqueSelect,
    UniqueOptions,
    UniqueRunning,
    UniqueDone,
}

/// Dedup configuration.
#[derive(Debug, Clone)]
pub struct DedupConfig {
    /// Enable Tier 2 (content hash) fallback.
    pub enable_tier2: bool,
    /// Body preview length for Tier 2 hash (bytes).
    pub body_hash_len: usize,
    /// Include attachment metadata in Tier 2 hash.
    pub include_attachments: bool,
    /// Output directory for reports and exports.
    pub output_dir: Option<PathBuf>,
}

impl Default for DedupConfig {
    fn default() -> Self {
        Self {
            enable_tier2: true,
            body_hash_len: 4096,
            include_attachments: true,
            output_dir: None,
        }
    }
}

/// The main application struct.
pub struct PstDedupApp {
    state: AppState,
    /// Selected PST file paths (legacy scan path).
    pst_files: Vec<PathBuf>,
    /// Dedup configuration.
    config: DedupConfig,
    /// Shared progress state (updated by worker thread).
    progress: Arc<Mutex<ScanProgress>>,
    /// Final results (set when scan completes).
    results: Option<ScanResult>,
    /// Handle to the worker thread.
    worker_handle: Option<std::thread::JoinHandle<ScanResult>>,
    /// Error message to display.
    error_msg: Option<String>,
    /// Last export result: (exported_count, failed_count, error).
    export_result: Option<(u64, u64, Option<String>)>,

    // ── Unique-PST wizard (0072) ────────────────────────────────────────────
    unique_form: UniqueWizardForm,
    unique_form_error: Option<String>,
    unique_progress: Arc<Mutex<UniqueProgressState>>,
    unique_cancel: Arc<AtomicBool>,
    unique_worker: Option<std::thread::JoinHandle<UniqueOutcomeView>>,
}

impl PstDedupApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        Self {
            state: AppState::FileSelect,
            pst_files: Vec::new(),
            config: DedupConfig::default(),
            progress: Arc::new(Mutex::new(ScanProgress::default())),
            results: None,
            worker_handle: None,
            error_msg: None,
            export_result: None,
            unique_form: UniqueWizardForm::default(),
            unique_form_error: None,
            unique_progress: Arc::new(Mutex::new(UniqueProgressState::default())),
            unique_cancel: Arc::new(AtomicBool::new(false)),
            unique_worker: None,
        }
    }

    /// Add PST files via native file dialog (main / COM-safe thread only).
    pub fn open_file_dialog(&mut self) {
        let files = rfd::FileDialog::new()
            .add_filter("PST Files", &["pst"])
            .set_title("Select PST Files")
            .pick_files();

        if let Some(paths) = files {
            for path in paths {
                if !self.pst_files.contains(&path) {
                    self.pst_files.push(path);
                }
            }
        }
    }

    /// Unique wizard: multi-select open dialog (main thread only — never from worker).
    pub fn open_unique_input_dialog(&mut self) {
        let files = rfd::FileDialog::new()
            .add_filter("PST Files", &["pst"])
            .set_title("Select source PST Files")
            .pick_files();
        if let Some(paths) = files {
            for path in paths {
                let path = crate::unique_wizard::absolutize_path(path);
                if !self.unique_form.inputs.contains(&path) {
                    self.unique_form.inputs.push(path);
                }
            }
        }
    }

    /// Unique wizard: Save File dialog for primary `.pst` out (main thread only).
    pub fn open_unique_out_dialog(&mut self) {
        let file = rfd::FileDialog::new()
            .add_filter("PST Files", &["pst"])
            .set_title("Save unique PST as…")
            .set_file_name("unique.pst")
            .save_file();
        if let Some(mut path) = file {
            if path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| !e.eq_ignore_ascii_case("pst"))
                .unwrap_or(true)
            {
                path.set_extension("pst");
            }
            self.unique_form.out = Some(crate::unique_wizard::absolutize_path(path));
        }
    }

    /// Unique wizard: report-dir folder picker (main thread only).
    pub fn open_unique_report_dir_dialog(&mut self) {
        let dir = rfd::FileDialog::new()
            .set_title("Select report pack folder")
            .pick_folder();
        if let Some(path) = dir {
            self.unique_form.report_dir = Some(crate::unique_wizard::absolutize_path(path));
        }
    }

    /// Enter unique wizard from file select (empty or with current scan files).
    pub fn enter_unique_wizard(&mut self) {
        let inputs = if !self.pst_files.is_empty() {
            self.pst_files.clone()
        } else {
            Vec::new()
        };
        self.unique_form = UniqueWizardForm::with_inputs(inputs);
        self.unique_form_error = None;
        self.state = AppState::UniqueSelect;
    }

    /// Enter unique wizard prefilled from scan results (primary unique export path).
    pub fn enter_unique_wizard_from_results(&mut self) {
        let inputs = self
            .results
            .as_ref()
            .map(|r| r.source_files.clone())
            .unwrap_or_else(|| self.pst_files.clone());
        self.unique_form = UniqueWizardForm::with_inputs(inputs);
        self.unique_form_error = None;
        self.state = AppState::UniqueSelect;
    }

    pub fn leave_unique_wizard(&mut self) {
        self.unique_form_error = None;
        // Cancel + join any in-flight worker so temps/report finalize before leave.
        self.unique_cancel.store(true, Ordering::SeqCst);
        if let Some(handle) = self.unique_worker.take() {
            let _ = handle.join();
        }
        self.state = AppState::FileSelect;
    }

    /// Start unique-pst on a background worker. `ctx` is cloned for repaint wakes.
    pub fn start_unique_pst(&mut self, ctx: egui::Context) {
        self.unique_form_error = None;
        if let Err(e) = self.unique_form.validate_for_run() {
            self.unique_form_error = Some(e);
            return;
        }
        let args = match self.unique_form.to_cli_args() {
            Ok(a) => a,
            Err(e) => {
                self.unique_form_error = Some(e);
                return;
            }
        };

        self.unique_cancel.store(false, Ordering::SeqCst);
        {
            let mut p = self
                .unique_progress
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            *p = UniqueProgressState::default();
        }

        let progress = Arc::clone(&self.unique_progress);
        let cancel = Arc::clone(&self.unique_cancel);
        let handle = unique_worker::spawn_unique_pst(args, progress, cancel, ctx);
        self.unique_worker = Some(handle);
        self.state = AppState::UniqueRunning;
    }

    pub fn cancel_unique_pst(&mut self) {
        self.unique_cancel.store(true, Ordering::SeqCst);
        let mut p = self
            .unique_progress
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        p.cancelled = true;
    }

    fn check_unique_worker(&mut self) {
        if let Some(handle) = &self.unique_worker {
            if handle.is_finished() {
                if let Some(handle) = self.unique_worker.take() {
                    match handle.join() {
                        Ok(_view) => {
                            // Outcome already stored in unique_progress by worker.
                            self.state = AppState::UniqueDone;
                        }
                        Err(_) => {
                            self.unique_form_error = Some("Unique-PST worker panicked".into());
                            {
                                let mut p = self
                                    .unique_progress
                                    .lock()
                                    .unwrap_or_else(|e| e.into_inner());
                                p.complete = true;
                                p.error = Some("worker panicked".into());
                            }
                            self.state = AppState::UniqueDone;
                        }
                    }
                }
            }
        }
    }

    /// Start the scan in a background thread.
    pub fn start_scan(&mut self) {
        let files = self.pst_files.clone();
        let config = self.config.clone();
        let progress = Arc::clone(&self.progress);

        // Reset progress
        {
            let mut p = progress.lock().unwrap_or_else(|e| e.into_inner());
            *p = ScanProgress::default();
        }

        self.state = AppState::Scanning;
        self.error_msg = None;

        let handle = std::thread::spawn(move || worker::run_scan(files, config, progress));

        self.worker_handle = Some(handle);
    }

    /// Check if the worker thread has finished.
    fn check_worker(&mut self) {
        if let Some(handle) = &self.worker_handle {
            if handle.is_finished() {
                if let Some(handle) = self.worker_handle.take() {
                    match handle.join() {
                        Ok(result) => {
                            self.results = Some(result);
                            self.state = AppState::Results;
                        }
                        Err(_) => {
                            self.error_msg = Some("Worker thread panicked".into());
                            self.state = AppState::FileSelect;
                        }
                    }
                }
            }
        }
    }

    /// Reset to file selection.
    pub fn reset(&mut self) {
        self.unique_cancel.store(true, Ordering::SeqCst);
        if let Some(handle) = self.unique_worker.take() {
            let _ = handle.join();
        }
        self.state = AppState::FileSelect;
        self.pst_files.clear();
        self.config = DedupConfig::default();
        self.results = None;
        self.error_msg = None;
        self.export_result = None;
        self.unique_form = UniqueWizardForm::default();
        self.unique_form_error = None;
        self.unique_cancel.store(false, Ordering::SeqCst);
    }

    /// Record the result of an EML export operation.
    pub fn set_export_result(&mut self, exported: u64, failed: u64, error: Option<String>) {
        self.export_result = Some((exported, failed, error));
    }
}

impl Drop for PstDedupApp {
    /// Window close / app teardown: cooperative-cancel a running unique-pst worker
    /// and join so report/temp cleanup can finish.
    ///
    /// Scan checks cancel between messages, so the join should exit promptly.
    /// Join panics are ignored (app is already tearing down).
    fn drop(&mut self) {
        self.unique_cancel.store(true, Ordering::SeqCst);
        if let Some(handle) = self.unique_worker.take() {
            let _ = handle.join();
        }
    }
}

impl eframe::App for PstDedupApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        // Check worker completion
        if self.state == AppState::Scanning {
            self.check_worker();
            // Request repaint while scanning for progress updates
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        if self.state == AppState::UniqueRunning {
            self.check_unique_worker();
            // Fallback repaint; worker also calls request_repaint on ticks.
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }

        // Top panel with title
        egui::Panel::top("header").show_inside(ui, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.heading("PST-Dedup");
                ui.label("—");
                ui.label(match self.state {
                    AppState::FileSelect => "Select PST Files",
                    AppState::Settings => "Configure",
                    AppState::Scanning => "Scanning...",
                    AppState::Results => "Results",
                    AppState::UniqueSelect => "Unique PST — Select",
                    AppState::UniqueOptions => "Unique PST — Options",
                    AppState::UniqueRunning => "Unique PST — Running",
                    AppState::UniqueDone => "Unique PST — Done",
                });
            });
            ui.add_space(4.0);
        });

        // Error banner
        if let Some(err) = &self.error_msg {
            egui::Panel::top("error").show_inside(ui, |ui| {
                ui.colored_label(egui::Color32::RED, format!("Error: {}", err));
            });
        }

        // Main content
        egui::CentralPanel::default().show_inside(ui, |ui| match self.state {
            AppState::FileSelect => views::file_select::show(ui, self),
            AppState::Settings => views::settings::show(ui, self),
            AppState::Scanning => views::progress::show(ui, self),
            AppState::Results => views::results::show(ui, self),
            AppState::UniqueSelect => views::unique_wizard::show_select(ui, self),
            AppState::UniqueOptions => views::unique_wizard::show_options(ui, self),
            AppState::UniqueRunning => views::unique_wizard::show_running(ui, self),
            AppState::UniqueDone => views::unique_wizard::show_done(ui, self),
        });
    }
}

// Make fields accessible to view modules within the crate.
impl PstDedupApp {
    pub fn set_state(&mut self, s: AppState) {
        self.state = s;
    }
    pub fn pst_files(&self) -> &[PathBuf] {
        &self.pst_files
    }
    pub fn pst_files_mut(&mut self) -> &mut Vec<PathBuf> {
        &mut self.pst_files
    }
    pub fn config(&self) -> &DedupConfig {
        &self.config
    }
    pub fn config_mut(&mut self) -> &mut DedupConfig {
        &mut self.config
    }
    pub fn progress(&self) -> &Arc<Mutex<ScanProgress>> {
        &self.progress
    }
    pub fn results(&self) -> Option<&ScanResult> {
        self.results.as_ref()
    }
    pub fn export_result(&self) -> Option<(u64, u64, Option<String>)> {
        self.export_result.clone()
    }
    pub fn unique_form(&self) -> &UniqueWizardForm {
        &self.unique_form
    }
    pub fn unique_form_mut(&mut self) -> &mut UniqueWizardForm {
        &mut self.unique_form
    }
    pub fn unique_form_error(&self) -> Option<&str> {
        self.unique_form_error.as_deref()
    }
    pub fn unique_progress(&self) -> &Arc<Mutex<UniqueProgressState>> {
        &self.unique_progress
    }
}
