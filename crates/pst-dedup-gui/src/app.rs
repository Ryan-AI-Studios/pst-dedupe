//! Top-level application state machine and egui App implementation.

use eframe::egui;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::views;
use crate::worker::{self, ScanProgress, ScanResult};

/// Application states.
#[derive(Debug, Clone, PartialEq)]
pub enum AppState {
    FileSelect,
    Settings,
    Scanning,
    Results,
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
    /// Selected PST file paths.
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
        }
    }

    /// Add PST files via native file dialog.
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

    /// Start the scan in a background thread.
    pub fn start_scan(&mut self) {
        let files = self.pst_files.clone();
        let config = self.config.clone();
        let progress = Arc::clone(&self.progress);

        // Reset progress
        {
            let mut p = progress.lock().unwrap();
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
        self.state = AppState::FileSelect;
        self.pst_files.clear();
        self.config = DedupConfig::default();
        self.results = None;
        self.error_msg = None;
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
        });
    }
}

// Make fields accessible to view modules within the crate.
impl PstDedupApp {
    pub fn state(&self) -> &AppState {
        &self.state
    }
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
}
