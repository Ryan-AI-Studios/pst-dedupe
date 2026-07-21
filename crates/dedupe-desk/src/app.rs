//! Top-level Dedupe Desk application state.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use camino::{Utf8Path, Utf8PathBuf};
use eframe::egui;
use matter_core::CaseOverview;
use process_runner::{register_default_handlers, JobParams, ProcessRunner, RunnerConfig};
use tokio::sync::watch;

use crate::cluster_ui::{self, ClusterState};
use crate::dialogs::{DialogKind, DialogState};
use crate::gap_ui::{self, GapState};
use crate::matter_ops::{MatterOpResult, MatterOpState};
use crate::matter_ui::{self, MatterSnapshot, OverviewLoadResult, OverviewLoadState};
use crate::nav::{self, Screen};
use crate::params::{self, format_runner_error, is_transient_sqlite_lock};
use crate::people_ui::{self, PeopleState};
use crate::produce_qc::{
    evaluate_produce_qc_readiness, hydrate_last_qc_summary, load_findings_csv, ProduceQcReadiness,
    QcFindingRow, FINDINGS_DISPLAY_CAP,
};
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
    /// Produce dialog open + draft fields.
    pub(crate) produce_dialog_open: bool,
    pub(crate) produce_name: String,
    pub(crate) produce_bates_prefix: String,
    pub(crate) produce_fail_if_withheld: bool,
    pub(crate) produce_expand_family: bool,
    pub(crate) produce_require_qc_pass: bool,
    pub(crate) produce_output_dir: String,
    /// Last production QC summary (from job message / status / hydrated qc_runs).
    pub(crate) last_qc_status: Option<String>,
    pub(crate) last_qc_report_path: Option<String>,
    pub(crate) last_qc_passed: Option<bool>,
    pub(crate) last_qc_error_count: Option<u64>,
    pub(crate) last_qc_warn_count: Option<u64>,
    /// Soft-gate readiness from matter_qc (freshness vs current selection).
    pub(crate) produce_qc_readiness: ProduceQcReadiness,
    /// Flags last used when computing `produce_qc_readiness` (detect checkbox drift).
    produce_qc_readiness_expand: bool,
    produce_qc_readiness_require: bool,
    /// Loaded findings.csv rows for the desk panel (capped).
    pub(crate) qc_findings: Vec<QcFindingRow>,
    pub(crate) qc_findings_error: Option<String>,
    pub(crate) qc_findings_show: bool,
    /// Review screen state (thin list + body loader).
    pub(crate) review: ReviewState,
    /// Gap analysis panel state (track 0042).
    pub(crate) gap: GapState,
    /// People–comms graph panel (track 0047).
    pub(crate) people: PeopleState,
    /// Concept / theme clusters panel (track 0048).
    pub(crate) clusters: ClusterState,
    /// Selected processing profile id (`builtin:standard` or user `pfl_…`).
    pub(crate) selected_profile_id: String,
    /// Draft name for Save profile as….
    pub(crate) profile_save_as_name: String,
    /// Selected workflow id (`builtin:reduce_only_chain` or user `wfl_…`).
    pub(crate) selected_workflow_id: String,
    /// Optional run_params for ingest/extract workflow placeholders.
    pub(crate) workflow_source_path: String,
    pub(crate) workflow_source_id: String,
    pub(crate) workflow_pst_item_id: String,
    /// Draft buffer for AI API key entry (never persisted in DeskSettings JSON).
    ai_api_key_draft: String,
}

impl DeskApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let mut runner = ProcessRunner::new(RunnerConfig::default());
        register_default_handlers(&mut runner);
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
            produce_dialog_open: false,
            produce_name: "Review Production".into(),
            produce_bates_prefix: "PROD".into(),
            produce_fail_if_withheld: false,
            produce_expand_family: false,
            produce_require_qc_pass: true,
            produce_output_dir: String::new(),
            last_qc_status: None,
            last_qc_report_path: None,
            last_qc_passed: None,
            last_qc_error_count: None,
            last_qc_warn_count: None,
            produce_qc_readiness: ProduceQcReadiness::Unknown,
            produce_qc_readiness_expand: false,
            produce_qc_readiness_require: true,
            qc_findings: Vec::new(),
            qc_findings_error: None,
            qc_findings_show: false,
            review: ReviewState::default(),
            gap: GapState::new(),
            people: PeopleState::new(),
            clusters: ClusterState::new(),
            selected_profile_id: params::PROFILE_DEFAULT_SELECTION.into(),
            profile_save_as_name: String::new(),
            selected_workflow_id: params::WORKFLOW_DEFAULT_SELECTION.into(),
            workflow_source_path: String::new(),
            workflow_source_id: String::new(),
            workflow_pst_item_id: String::new(),
            ai_api_key_draft: String::new(),
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
        self.last_qc_status = None;
        self.last_qc_report_path = None;
        self.last_qc_passed = None;
        self.last_qc_error_count = None;
        self.last_qc_warn_count = None;
        self.produce_qc_readiness = ProduceQcReadiness::Unknown;
        self.qc_findings.clear();
        self.qc_findings_error = None;
        self.qc_findings_show = false;
        self.hydrate_qc_from_matter();
        self.hydrate_ai_settings_from_matter();
        self.hydrate_lang_pack_from_matter();
        self.refresh_produce_qc_readiness();
        self.refresh_matter_lists();
        self.status_msg = Some(format!("Opened matter at {root}"));
    }

    /// Copy matter language pack into Desk settings (FTS pack select).
    fn hydrate_lang_pack_from_matter(&mut self) {
        let Some(root) = self.matter_root.as_ref() else {
            return;
        };
        match matter_core::Matter::open_for_read(root.as_path()) {
            Ok(matter) => match matter.get_lang_config() {
                Ok(cfg) => {
                    self.settings.lang_pack_id = cfg.lang_pack_id;
                    self.settings.save();
                }
                Err(e) => {
                    self.error_msg = Some(format!("Could not load matter language pack: {e}"));
                }
            },
            Err(e) => {
                self.error_msg = Some(format!(
                    "Could not open matter for language pack hydrate: {e}"
                ));
            }
        }
    }

    /// Dual-write language pack to open matter; clears FTS fingerprint (rebuild required).
    fn dual_write_lang_pack(&mut self) {
        let Some(root) = self.matter_root.as_ref() else {
            self.settings.save();
            return;
        };
        let pack = self.settings.lang_pack_id.clone();
        match matter_core::Matter::open(root.as_path()) {
            Ok(matter) => match matter.update_lang_pack(&pack) {
                Ok(cfg) => {
                    self.settings.lang_pack_id = cfg.lang_pack_id;
                    self.settings.save();
                    self.status_msg = Some(
                        "Language pack updated — Rebuild FTS required before keyword search."
                            .into(),
                    );
                    self.review.keyword_error = Some(
                        "Index is stale due to language pack change. Rebuild required. (fts_lang_pack_stale)"
                            .into(),
                    );
                }
                Err(e) => {
                    self.error_msg = Some(format!("Could not update language pack: {e}"));
                }
            },
            Err(e) => {
                self.error_msg = Some(format!("Could not open matter to save language pack: {e}"));
            }
        }
    }

    /// Copy matter AI config into Desk settings so the UI matches the DB on open.
    /// Dual-write on user edit (`dual_write_ai_config`) remains the write path.
    fn hydrate_ai_settings_from_matter(&mut self) {
        let Some(root) = self.matter_root.as_ref() else {
            return;
        };
        match matter_core::Matter::open_for_read(root.as_path()) {
            Ok(matter) => match matter.get_ai_config() {
                Ok(cfg) => {
                    self.settings.ai_enabled = cfg.ai_enabled;
                    self.settings.ai_allow_remote = cfg.ai_allow_remote;
                    self.settings.ai_base_url = cfg.ai_base_url;
                    self.settings.ai_model = cfg.ai_model;
                    self.settings.ai_provider_kind = cfg.ai_provider_kind;
                    // Persist so a cold restart after open still reflects matter.
                    self.settings.save();
                }
                Err(e) => {
                    self.error_msg = Some(format!("Could not load matter AI settings: {e}"));
                }
            },
            Err(e) => {
                self.error_msg = Some(format!(
                    "Could not open matter for AI settings hydrate: {e}"
                ));
            }
        }
    }

    /// Hydrate session QC fields from `qc_runs` so reopen is not forced re-QC when fresh.
    fn hydrate_qc_from_matter(&mut self) {
        let Some(root) = self.matter_root.as_ref() else {
            return;
        };
        let h = hydrate_last_qc_summary(root);
        if h.passed.is_some() {
            self.last_qc_passed = h.passed;
            self.last_qc_error_count = h.error_count;
            self.last_qc_warn_count = h.warn_count;
            self.last_qc_report_path = h.report_path;
            self.last_qc_status = h.status;
            if let Some(ref path) = self.last_qc_report_path.clone() {
                self.load_qc_findings_from_report(path);
            }
        }
    }

    /// Recompute soft-gate using current produce flags (cheap open_for_read + SQL).
    pub(crate) fn refresh_produce_qc_readiness(&mut self) {
        let Some(root) = self.matter_root.as_ref() else {
            self.produce_qc_readiness = ProduceQcReadiness::Unknown;
            return;
        };
        self.produce_qc_readiness_expand = self.produce_expand_family;
        self.produce_qc_readiness_require = self.produce_require_qc_pass;
        self.produce_qc_readiness = evaluate_produce_qc_readiness(
            root,
            self.produce_require_qc_pass,
            self.produce_expand_family,
        );
    }

    /// Load findings.csv into the scrollable panel (cap [`FINDINGS_DISPLAY_CAP`]).
    fn load_qc_findings_from_report(&mut self, report_path: &str) {
        match load_findings_csv(report_path, FINDINGS_DISPLAY_CAP) {
            Ok(rows) => {
                self.qc_findings = rows;
                self.qc_findings_error = None;
                self.qc_findings_show = !self.qc_findings.is_empty();
            }
            Err(e) => {
                self.qc_findings.clear();
                self.qc_findings_error = Some(e);
                self.qc_findings_show = false;
            }
        }
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
        self.report_export_status = Some("Choose folder or cancel for default…".into());
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

    /// Navigate to Review and select a specific item id (QC findings → Review).
    pub(crate) fn open_review_item(&mut self, item_id: &str) {
        if self.matter_root.is_none() {
            return;
        }
        let id = item_id.trim();
        if id.is_empty() {
            self.open_review();
            return;
        }
        self.review.request_jump_to_item(id);
        self.screen = Screen::Review;
        self.status_msg = Some(format!("Review: jump to {id}"));
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

    /// Open the Produce review set dialog (defaults under exports/productions).
    pub(crate) fn open_produce_dialog(&mut self) {
        if self.matter_root.is_none() {
            self.error_msg = Some("No matter open.".into());
            return;
        }
        if self.produce_name.trim().is_empty() {
            self.produce_name = "Review Production".into();
        }
        if self.produce_bates_prefix.trim().is_empty() {
            self.produce_bates_prefix = "PROD".into();
        }
        // Default empty output → engine uses exports/productions/<name_or_stamp>/.
        self.refresh_produce_qc_readiness();
        self.produce_dialog_open = true;
    }

    /// Start production QC job on the review corpus.
    ///
    /// Uses the same `expand_family` flag as produce so the QC selection
    /// fingerprint matches produce when Require QC pass is on.
    pub(crate) fn start_production_qc(&mut self) {
        let Some(root) = self.matter_root.clone() else {
            self.error_msg = Some("No matter open.".into());
            return;
        };
        let params_json = params::qc_params("review_corpus", self.produce_expand_family, None);
        let params = JobParams::new(params_json);
        match self
            .runner
            .start(Utf8Path::new(root.as_str()), "qc", params)
        {
            Ok(job_id) => {
                self.last_job_id = Some(job_id.clone());
                self.status_msg = Some(format!(
                    "Started production QC job {job_id} (expand_family={})",
                    self.produce_expand_family
                ));
                self.error_msg = None;
                self.last_qc_status = Some("running…".into());
            }
            Err(e) => {
                self.error_msg = Some(format_runner_error(&e));
            }
        }
    }

    /// Start collection gap analysis job (`kind = "gap"`).
    pub(crate) fn start_collection_gap(&mut self) {
        let Some(root) = self.matter_root.clone() else {
            self.error_msg = Some("No matter open.".into());
            return;
        };
        let params_json = gap_ui::collection_params_from_state(&self.gap);
        let params = JobParams::new(params_json);
        match self
            .runner
            .start(Utf8Path::new(root.as_str()), "gap", params)
        {
            Ok(job_id) => {
                self.last_job_id = Some(job_id.clone());
                self.status_msg = Some(format!("Started collection gap job {job_id}"));
                self.error_msg = None;
                self.gap.last_status = Some("running collection gap…".into());
            }
            Err(e) => {
                self.error_msg = Some(format_runner_error(&e));
            }
        }
    }

    /// Start opposing DAT set-diff job (`kind = "gap"`).
    pub(crate) fn start_opposing_gap(&mut self) {
        let Some(root) = self.matter_root.clone() else {
            self.error_msg = Some("No matter open.".into());
            return;
        };
        let Some(params_json) = gap_ui::opposing_params_from_state(&self.gap) else {
            self.error_msg = Some("Import an opposing DAT first.".into());
            return;
        };
        let params = JobParams::new(params_json);
        match self
            .runner
            .start(Utf8Path::new(root.as_str()), "gap", params)
        {
            Ok(job_id) => {
                self.last_job_id = Some(job_id.clone());
                self.status_msg = Some(format!("Started opposing gap job {job_id}"));
                self.error_msg = None;
                self.gap.last_status = Some("running opposing compare…".into());
            }
            Err(e) => {
                self.error_msg = Some(format_runner_error(&e));
            }
        }
    }

    /// Capture last gap outcome from runner progress message.
    pub(crate) fn note_gap_progress_message(&mut self, message: &str) {
        if !message.contains("gap kind=") && !message.starts_with("gap ") {
            return;
        }
        self.gap.last_status = Some(message.to_string());
        if let Some(idx) = message.find('→') {
            let path = message[idx + '→'.len_utf8()..].trim();
            if !path.is_empty() {
                self.gap.last_report_path = Some(path.to_string());
            }
        } else if let Some(idx) = message.find("->") {
            let path = message[idx + 2..].trim();
            if !path.is_empty() {
                self.gap.last_report_path = Some(path.to_string());
            }
        }
    }

    /// Capture last QC outcome from runner progress message (best-effort parse).
    pub(crate) fn note_qc_progress_message(&mut self, message: &str) {
        // Example: "qc passed=true errors=0 warns=1 candidates=12 → C:\...\qc_..."
        if !message.contains("qc passed=") && !message.contains("errors=") {
            return;
        }
        self.last_qc_status = Some(message.to_string());
        if let Some(idx) = message.find("passed=") {
            let rest = &message[idx + "passed=".len()..];
            let token = rest.split_whitespace().next().unwrap_or("");
            self.last_qc_passed = Some(token.starts_with("true") || token.starts_with('1'));
        }
        if let Some(idx) = message.find("errors=") {
            let rest = &message[idx + "errors=".len()..];
            if let Some(n) = rest.split([' ', '=']).next().and_then(|s| s.parse().ok()) {
                self.last_qc_error_count = Some(n);
            }
        }
        if let Some(idx) = message.find("warns=") {
            let rest = &message[idx + "warns=".len()..];
            if let Some(n) = rest.split([' ', '=']).next().and_then(|s| s.parse().ok()) {
                self.last_qc_warn_count = Some(n);
            }
        }
        if let Some(idx) = message.find('→') {
            let path = message[idx + '→'.len_utf8()..].trim();
            if !path.is_empty() {
                self.last_qc_report_path = Some(path.to_string());
            }
        } else if let Some(idx) = message.find("->") {
            let path = message[idx + 2..].trim();
            if !path.is_empty() {
                self.last_qc_report_path = Some(path.to_string());
            }
        }
        if let Some(ref path) = self.last_qc_report_path.clone() {
            self.load_qc_findings_from_report(path);
        }
        self.refresh_produce_qc_readiness();
    }

    /// Start produce job from dialog draft fields.
    ///
    /// Always re-evaluates QC soft-gate immediately before start (fail closed).
    pub(crate) fn start_produce(&mut self) {
        let Some(root) = self.matter_root.clone() else {
            self.error_msg = Some("No matter open.".into());
            return;
        };
        // Fail closed: re-check readiness at click time (selection may have mutated).
        self.refresh_produce_qc_readiness();
        if self.produce_require_qc_pass && !self.produce_qc_readiness.allows_produce() {
            self.error_msg = Some(format!(
                "Produce blocked: {}",
                self.produce_qc_readiness.label()
            ));
            return;
        }
        let name = self.produce_name.trim();
        let prefix = self.produce_bates_prefix.trim();
        if name.is_empty() {
            self.error_msg = Some("Production name is required.".into());
            return;
        }
        if prefix.is_empty() {
            self.error_msg = Some("Bates prefix is required.".into());
            return;
        }
        let output = self.produce_output_dir.trim();
        let output_opt = if output.is_empty() {
            None
        } else {
            Some(output)
        };
        let params_json = params::produce_params(
            name,
            prefix,
            self.produce_fail_if_withheld,
            self.produce_expand_family,
            self.produce_require_qc_pass,
            output_opt,
        );
        let params = JobParams::new(params_json);
        match self
            .runner
            .start(Utf8Path::new(root.as_str()), "produce", params)
        {
            Ok(job_id) => {
                self.last_job_id = Some(job_id.clone());
                self.status_msg = Some(format!(
                    "Started produce job {job_id} (name={name}, prefix={prefix})"
                ));
                self.error_msg = None;
                self.produce_dialog_open = false;
            }
            Err(e) => self.note_start_error(e),
        }
    }

    /// Open the last QC report directory in the OS file manager (Windows Explorer).
    pub(crate) fn open_qc_findings_folder(&mut self) {
        let Some(ref path) = self.last_qc_report_path else {
            self.error_msg = Some("No QC report path available.".into());
            return;
        };
        let p = Utf8Path::new(path.as_str());
        if !p.exists() {
            self.error_msg = Some(format!("QC report folder not found: {path}"));
            return;
        }
        match open_folder_in_explorer(p.as_str()) {
            Ok(()) => {
                self.status_msg = Some(format!("Opened findings folder: {path}"));
                self.error_msg = None;
            }
            Err(e) => {
                self.error_msg = Some(format!("Could not open findings folder: {e}"));
            }
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

    /// Tooltip / enablement for Workspace **Run transcription**.
    pub(crate) fn stt_run_tooltip(&self) -> String {
        if !self.settings.stt_enabled {
            return "Enable local STT in Settings (Home) first — off by default".into();
        }
        if !self.stt_tools_look_available() {
            return "whisper-cli and/or model not found — set paths in Settings (no silent download)"
                .into();
        }
        "Run local offline STT on audio/video natives (kind=transcribe). Un-diarized — verify by listening. Rebuild FTS after.".into()
    }

    /// True when STT is enabled and whisper CLI + model appear resolvable.
    pub(crate) fn stt_run_enabled(&self) -> bool {
        self.settings.stt_enabled && self.stt_tools_look_available()
    }

    fn stt_tools_look_available(&self) -> bool {
        let cli_ok = if let Some(p) = self.settings.whisper_cli_path.as_deref().map(str::trim) {
            if !p.is_empty() {
                std::path::Path::new(p).is_file()
            } else {
                which_on_path("whisper-cli")
                    || which_on_path("whisper-cli.exe")
                    || which_on_path("whisper")
                    || which_on_path("whisper.exe")
            }
        } else {
            which_on_path("whisper-cli")
                || which_on_path("whisper-cli.exe")
                || which_on_path("whisper")
                || which_on_path("whisper.exe")
        };
        let model_ok = self
            .settings
            .stt_model_path
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|p| std::path::Path::new(p).is_file())
            .unwrap_or(false);
        cli_ok && model_ok
    }

    /// Run local STT (`transcribe`) — fails closed when Settings STT is disabled.
    pub(crate) fn start_transcribe(&mut self) {
        let Some(root) = self.matter_root.clone() else {
            self.error_msg = Some("No matter open.".into());
            return;
        };
        if !self.settings.stt_enabled {
            self.error_msg =
                Some("STT is disabled. Enable local STT in Settings (Home) before running.".into());
            return;
        }
        if !self.stt_tools_look_available() {
            self.error_msg = Some(
                "whisper-cli or model not found. Set paths in Settings; models are never downloaded automatically."
                    .into(),
            );
            return;
        }
        let params = JobParams::new(params::transcribe_default_params(
            self.settings.stt_enabled,
            self.settings.whisper_cli_path.as_deref(),
            self.settings.stt_model_path.as_deref(),
            self.settings.ffmpeg_path.as_deref(),
        ));
        match self
            .runner
            .start(Utf8Path::new(root.as_str()), "transcribe", params)
        {
            Ok(job_id) => {
                self.last_job_id = Some(job_id.clone());
                self.status_msg = Some(format!("Started transcription job {job_id}"));
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

    /// Offline entity / PII pack scan (`entity_scan`).
    pub(crate) fn start_entity_scan(&mut self) {
        let Some(root) = self.matter_root.clone() else {
            self.error_msg = Some("No matter open.".into());
            return;
        };
        let params = JobParams::new(params::entity_scan_default_params());
        match self
            .runner
            .start(Utf8Path::new(root.as_str()), "entity_scan", params)
        {
            Ok(job_id) => {
                self.last_job_id = Some(job_id.clone());
                self.status_msg = Some(format!("Started entity_scan job {job_id}"));
                self.error_msg = None;
            }
            Err(e) => self.note_start_error(e),
        }
    }

    /// Offline sentiment / tone (`sentiment`) — VADER lexicon heuristic.
    pub(crate) fn start_sentiment(&mut self) {
        let Some(root) = self.matter_root.clone() else {
            self.error_msg = Some("No matter open.".into());
            return;
        };
        let params = JobParams::new(params::sentiment_default_params());
        match self
            .runner
            .start(Utf8Path::new(root.as_str()), "sentiment", params)
        {
            Ok(job_id) => {
                self.last_job_id = Some(job_id.clone());
                self.status_msg = Some(format!("Started sentiment job {job_id}"));
                self.error_msg = None;
            }
            Err(e) => self.note_start_error(e),
        }
    }

    /// Incremental semantic index build/update (`semantic_index`, `reset: false`).
    pub(crate) fn start_semantic_index(&mut self) {
        let Some(root) = self.matter_root.clone() else {
            self.error_msg = Some("No matter open.".into());
            return;
        };
        let params = JobParams::new(params::semantic_index_default_params());
        match self
            .runner
            .start(Utf8Path::new(root.as_str()), "semantic_index", params)
        {
            Ok(job_id) => {
                self.last_job_id = Some(job_id.clone());
                self.status_msg = Some(format!("Started semantic_index job {job_id}"));
                self.error_msg = None;
                self.review.semantic_error = None;
            }
            Err(e) => self.note_start_error(e),
        }
    }

    /// Full semantic index rebuild (`reset: true`).
    pub(crate) fn start_semantic_rebuild(&mut self) {
        let Some(root) = self.matter_root.clone() else {
            self.error_msg = Some("No matter open.".into());
            return;
        };
        self.review.semantic_error = None;
        let params = JobParams::new(params::semantic_index_reset_params());
        match self
            .runner
            .start(Utf8Path::new(root.as_str()), "semantic_index", params)
        {
            Ok(job_id) => {
                self.last_job_id = Some(job_id.clone());
                self.status_msg = Some(format!(
                    "Started semantic_index rebuild job {job_id} (reset:true)"
                ));
                self.error_msg = None;
            }
            Err(e) => self.note_start_error(e),
        }
    }

    /// First-pass AI code suggestions (`ai_suggest_codes`) — suggestions only.
    pub(crate) fn start_ai_suggest_codes(&mut self) {
        let Some(root) = self.matter_root.clone() else {
            self.error_msg = Some("No matter open.".into());
            return;
        };
        if !self.settings.ai_enabled {
            self.error_msg = Some(
                "AI is off. Enable AI in Settings (and configure provider) before running suggestions."
                    .into(),
            );
            return;
        }
        let params = JobParams::new(params::ai_suggest_codes_default_params());
        match self
            .runner
            .start(Utf8Path::new(root.as_str()), "ai_suggest_codes", params)
        {
            Ok(job_id) => {
                self.last_job_id = Some(job_id.clone());
                self.status_msg = Some(format!("Started ai_suggest_codes job {job_id}"));
                self.error_msg = None;
            }
            Err(e) => self.note_start_error(e),
        }
    }

    /// Dual-write desk AI settings to open matter config when possible.
    pub(crate) fn dual_write_ai_config(&mut self) {
        self.settings.save();
        let Some(root) = self.matter_root.as_ref() else {
            return;
        };
        match matter_core::Matter::open(root.as_path()) {
            Ok(matter) => {
                let kind = self
                    .settings
                    .ai_provider_kind
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .unwrap_or(if self.settings.ai_enabled {
                        "mock"
                    } else {
                        "none"
                    });
                if let Err(e) = matter.update_ai_config(matter_core::UpdateAiMatterConfigInput {
                    enabled: self.settings.ai_enabled,
                    allow_remote: self.settings.ai_allow_remote,
                    base_url: self.settings.ai_base_url.as_deref(),
                    model: self.settings.ai_model.as_deref(),
                    provider_kind: Some(kind),
                }) {
                    self.error_msg = Some(format!("Could not update matter AI config: {e}"));
                }
            }
            Err(e) => {
                self.error_msg = Some(format!("Could not open matter for AI dual-write: {e}"));
            }
        }
    }

    /// Dual-write desk `semantic_enabled` preference to open matter meta when possible.
    pub(crate) fn dual_write_semantic_enabled(&mut self, enabled: bool) {
        self.settings.semantic_enabled = enabled;
        self.settings.save();
        let Some(root) = self.matter_root.as_ref() else {
            return;
        };
        match matter_core::Matter::open(root.as_path()) {
            Ok(matter) => {
                let meta = match matter.get_semantic_meta() {
                    Ok(m) => m,
                    Err(e) => {
                        self.error_msg =
                            Some(format!("Could not read semantic meta for dual-write: {e}"));
                        return;
                    }
                };
                if let Err(e) =
                    matter.update_semantic_matter_meta(matter_core::UpdateSemanticMatterMetaInput {
                        enabled,
                        model_id: meta.semantic_model_id.as_deref(),
                        dims: meta.semantic_dims,
                        chunk_params_json: meta.semantic_chunk_params_json.as_deref(),
                        fingerprint: meta.semantic_fingerprint.as_deref(),
                        built_at: meta.semantic_built_at.as_deref(),
                        job_id: meta.semantic_job_id.as_deref(),
                        chunk_count: meta.semantic_chunk_count,
                    })
                {
                    self.error_msg = Some(format!("Could not update matter semantic_enabled: {e}"));
                }
            }
            Err(e) => {
                self.error_msg = Some(format!(
                    "Could not open matter for semantic dual-write: {e}"
                ));
            }
        }
    }

    /// Offline people–comms graph build (`people_graph`).
    pub(crate) fn start_people_graph(&mut self) {
        let Some(root) = self.matter_root.clone() else {
            self.error_msg = Some("No matter open.".into());
            return;
        };
        let params = JobParams::new(params::people_graph_default_params());
        match self
            .runner
            .start(Utf8Path::new(root.as_str()), "people_graph", params)
        {
            Ok(job_id) => {
                self.last_job_id = Some(job_id.clone());
                self.status_msg = Some(format!("Started people_graph job {job_id}"));
                self.error_msg = None;
            }
            Err(e) => self.note_start_error(e),
        }
    }

    /// Offline concept clustering (`concept_cluster`).
    pub(crate) fn start_concept_cluster(&mut self) {
        let Some(root) = self.matter_root.clone() else {
            self.error_msg = Some("No matter open.".into());
            return;
        };
        let k = self.clusters.requested_k();
        let params = JobParams::new(params::concept_cluster_default_params(k));
        match self
            .runner
            .start(Utf8Path::new(root.as_str()), "concept_cluster", params)
        {
            Ok(job_id) => {
                self.last_job_id = Some(job_id.clone());
                self.status_msg = Some(format!("Started concept_cluster job {job_id} (k={k})"));
                self.error_msg = None;
            }
            Err(e) => self.note_start_error(e),
        }
    }

    /// Run the selected processing profile (`profile_run`).
    pub(crate) fn start_profile_run(&mut self) {
        let Some(root) = self.matter_root.clone() else {
            self.error_msg = Some("No matter open.".into());
            return;
        };
        let params = JobParams::new(params::profile_run_params(&self.selected_profile_id));
        match self
            .runner
            .start(Utf8Path::new(root.as_str()), "profile_run", params)
        {
            Ok(job_id) => {
                self.last_job_id = Some(job_id.clone());
                self.status_msg = Some(format!(
                    "Started profile_run ({}) job {job_id}",
                    self.selected_profile_id
                ));
                self.error_msg = None;
            }
            Err(e) => self.note_start_error(e),
        }
    }

    /// Build optional `run_params` object from desk workflow fields (AST bind placeholders).
    fn workflow_run_params_value(&self) -> serde_json::Value {
        let mut map = serde_json::Map::new();
        let path = self.workflow_source_path.trim();
        if !path.is_empty() {
            map.insert(
                "source_path".into(),
                serde_json::Value::String(path.to_string()),
            );
        }
        let source_id = self.workflow_source_id.trim();
        if !source_id.is_empty() {
            map.insert(
                "source_id".into(),
                serde_json::Value::String(source_id.to_string()),
            );
        }
        let pst_item_id = self.workflow_pst_item_id.trim();
        if !pst_item_id.is_empty() {
            map.insert(
                "pst_item_id".into(),
                serde_json::Value::String(pst_item_id.to_string()),
            );
        }
        // qc_then_produce uses empty node params + handler defaults; produce scope
        // toggles remain on the Produce screen (not injected here unless fields set).
        serde_json::Value::Object(map)
    }

    /// Run the selected workflow (`workflow_run`).
    pub(crate) fn start_workflow_run(&mut self) {
        let Some(root) = self.matter_root.clone() else {
            self.error_msg = Some("No matter open.".into());
            return;
        };
        let run_params = self.workflow_run_params_value();
        let params = JobParams::new(params::workflow_run_params(
            &self.selected_workflow_id,
            &run_params,
        ));
        match self
            .runner
            .start(Utf8Path::new(root.as_str()), "workflow_run", params)
        {
            Ok(job_id) => {
                self.last_job_id = Some(job_id.clone());
                self.status_msg = Some(format!(
                    "Started workflow_run ({}) job {job_id}",
                    self.selected_workflow_id
                ));
                self.error_msg = None;
            }
            Err(e) => self.note_start_error(e),
        }
    }

    /// Apply selected profile stage defaults to workspace toggles (cull/promote/OCR).
    pub(crate) fn apply_profile_defaults(&mut self) {
        let Some(root) = self.matter_root.clone() else {
            self.error_msg = Some("No matter open.".into());
            return;
        };
        let matter = match matter_core::Matter::open_for_read(Utf8Path::new(root.as_str())) {
            Ok(m) => m,
            Err(e) => {
                self.error_msg = Some(format!("Open matter failed: {e}"));
                return;
            }
        };
        let profile = match matter.get_processing_profile(&self.selected_profile_id) {
            Ok(p) => p,
            Err(e) => {
                self.error_msg = Some(format!("Load profile failed: {e}"));
                return;
            }
        };

        if let Some(cull) = profile.body.stages.get("cull") {
            if cull.enabled {
                if let Some(name) = cull.params.get("preset_name").and_then(|v| v.as_str()) {
                    self.cull_preset = name.to_string();
                } else if let Some(id) = cull.params.get("preset_id").and_then(|v| v.as_str()) {
                    self.cull_preset = format!("{}{}", params::CULL_USER_PRESET_PREFIX, id);
                }
            }
        }
        if let Some(promote) = profile.body.stages.get("promote") {
            if promote.enabled {
                if let Some(policy) = promote.params.get("policy").and_then(|v| v.as_str()) {
                    self.promote_policy = policy.to_string();
                }
            }
        }
        if let Some(ocr) = profile.body.stages.get("ocr") {
            // Outer enabled drives Settings OCR toggle for desk one-off Run OCR.
            let nested = ocr
                .params
                .get("enabled")
                .and_then(|v| v.as_bool())
                .unwrap_or(ocr.enabled);
            self.settings.ocr_enabled = ocr.enabled && nested;
            self.settings.save();
        }

        self.status_msg = Some(format!("Applied defaults from profile '{}'", profile.name));
        self.error_msg = None;
    }

    /// Save current workspace toggles as a new user processing profile.
    pub(crate) fn save_profile_as(&mut self) {
        let Some(root) = self.matter_root.clone() else {
            self.error_msg = Some("No matter open.".into());
            return;
        };
        let name = self.profile_save_as_name.trim().to_string();
        if name.is_empty() {
            self.error_msg = Some("Enter a name for Save profile as…".into());
            return;
        }
        if params::PROFILE_BUILTIN_NAMES.iter().any(|n| *n == name) {
            self.error_msg = Some(format!("'{name}' is reserved for a built-in profile."));
            return;
        }

        // Clone selected profile body and stamp desk cull/promote/OCR onto it.
        let matter = match matter_core::Matter::open(Utf8Path::new(root.as_str())) {
            Ok(m) => m,
            Err(e) => {
                self.error_msg = Some(format!("Open matter failed: {e}"));
                return;
            }
        };
        let base = match matter.get_processing_profile(&self.selected_profile_id) {
            Ok(p) => p,
            Err(e) => {
                self.error_msg = Some(format!("Load profile failed: {e}"));
                return;
            }
        };
        let mut body = base.body;
        if let Some(cull) = body.stages.get_mut("cull") {
            if let Some(id) = self
                .cull_preset
                .strip_prefix(params::CULL_USER_PRESET_PREFIX)
            {
                cull.params = serde_json::json!({
                    "preset_id": id,
                    "reset": false,
                    "batch_size": 500
                });
            } else {
                cull.params = serde_json::json!({
                    "preset_name": self.cull_preset,
                    "reset": false,
                    "batch_size": 500
                });
            }
            cull.enabled = true;
        }
        if let Some(promote) = body.stages.get_mut("promote") {
            promote.params = serde_json::json!({
                "policy": self.promote_policy,
                "review_set_name": "Review Corpus",
                "expand_families": true,
                "reset": false,
                "batch_size": 500,
                "require_dedupe": false
            });
            promote.enabled = true;
        }
        if let Some(ocr) = body.stages.get_mut("ocr") {
            ocr.enabled = self.settings.ocr_enabled;
            if let Some(obj) = ocr.params.as_object_mut() {
                obj.insert(
                    "enabled".into(),
                    serde_json::Value::Bool(self.settings.ocr_enabled),
                );
            }
        }

        let body_json = match matter_core::profile_body_to_json(&body) {
            Ok(j) => j,
            Err(e) => {
                self.error_msg = Some(format!("Serialize profile: {e}"));
                return;
            }
        };
        match matter.upsert_processing_profile(matter_core::ProcessingProfileInput {
            id: None,
            name: name.clone(),
            description: Some(format!("Saved from desk ({})", self.selected_profile_id)),
            body_json,
            created_by: Some("desk".into()),
        }) {
            Ok(saved) => {
                self.selected_profile_id = saved.id.clone();
                self.profile_save_as_name.clear();
                self.status_msg = Some(format!("Saved profile '{}' ({})", saved.name, saved.id));
                self.error_msg = None;
                self.refresh_matter_lists();
            }
            Err(e) => {
                self.error_msg = Some(format!("Save profile failed: {e}"));
            }
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

    /// Normalize Teams/chat export leaves (`teams_extract`).
    pub(crate) fn start_teams_extract(&mut self) {
        let Some(root) = self.matter_root.clone() else {
            self.error_msg = Some("No matter open.".into());
            return;
        };
        let params = JobParams::new(params::teams_extract_default_params());
        match self
            .runner
            .start(Utf8Path::new(root.as_str()), "teams_extract", params)
        {
            Ok(job_id) => {
                self.last_job_id = Some(job_id.clone());
                self.status_msg = Some(format!("Started Teams/chat extract job {job_id}"));
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
            // Capture QC / gap summary from terminal job messages.
            if prev == "running"
                && (state == "succeeded" || state == "failed")
                && snap.stage.as_deref() == Some("qc")
            {
                if let Some(ref msg) = snap.message {
                    self.note_qc_progress_message(msg);
                }
            } else if prev == "running"
                && (state == "succeeded" || state == "failed")
                && snap.stage.as_deref() == Some("gap")
            {
                if let Some(ref msg) = snap.message {
                    self.note_gap_progress_message(msg);
                }
            } else if prev == "running" && (state == "succeeded" || state == "failed") {
                // Fallback: message itself indicates QC / gap outcome.
                if let Some(ref msg) = snap.message {
                    if msg.contains("qc passed=") {
                        self.note_qc_progress_message(msg);
                    } else if msg.contains("gap kind=") {
                        self.note_gap_progress_message(msg);
                    }
                }
            }
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
                    // Selection-affecting jobs (promote/cull/etc.) may stale the QC gate.
                    self.refresh_produce_qc_readiness();
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
        ui.heading("Language pack (keyword FTS)");
        ui.label(
            egui::RichText::new(
                "Offline packs only — not machine translation. \
                 CJK pack uses character bigrams + phrase adjacency for consecutive CJK. \
                 Changing pack hard-blocks keyword search until Rebuild FTS.",
            )
            .weak()
            .small(),
        );
        let pack_locked = self.job_may_be_writing();
        ui.horizontal(|ui| {
            ui.label("Pack:");
            let mut pack = self.settings.lang_pack_id.clone();
            ui.add_enabled_ui(!pack_locked, |ui| {
                for (label, val) in [
                    ("latin_default", "latin_default"),
                    ("cjk_ngram_v1", "cjk_ngram_v1"),
                ] {
                    if ui
                        .selectable_label(pack == val, label)
                        .on_hover_text(if val == "cjk_ngram_v1" {
                            "Hybrid CJK n-gram + Latin/email-safe path"
                        } else {
                            "English-friendly Tantivy default tokenizer"
                        })
                        .clicked()
                    {
                        pack = val.into();
                    }
                }
            });
            if pack_locked {
                ui.label(
                    egui::RichText::new("(locked while a job runs)")
                        .weak()
                        .small(),
                );
            }
            if !pack_locked && self.settings.lang_pack_id != pack {
                self.settings.lang_pack_id = pack;
                self.dual_write_lang_pack();
            }
        });
        if ui
            .add_enabled(!pack_locked, egui::Button::new("Rebuild FTS"))
            .on_hover_text("Full FTS rebuild with the active language pack (reset:true)")
            .clicked()
        {
            self.start_fts_rebuild();
        }

        ui.add_space(8.0);
        ui.heading("Local semantic search (optional)");
        let mut semantic_on = self.settings.semantic_enabled;
        let sem_resp = ui.checkbox(
            &mut semantic_on,
            "Enable semantic search (local embeddings)",
        );
        if sem_resp.changed() {
            self.dual_write_semantic_enabled(semantic_on);
        }
        ui.label(
            egui::RichText::new(
                "Off by default. Additive to keyword FTS — not a replacement. \
                 Default embedder is mock:hash_v1 (no model weights). \
                 Run Build semantic index from Workspace or Review before searching.",
            )
            .weak()
            .small(),
        );

        ui.add_space(8.0);
        ui.heading("AI coding assist (optional)");
        ui.label(
            egui::RichText::new(
                "Suggestions only — may be wrong. Human accept required for final codes. \
                 Cloud sends matter text if remote is allowed. Not a substitute for privilege review.",
            )
            .color(egui::Color32::from_rgb(180, 120, 40))
            .small(),
        );
        let mut ai_on = self.settings.ai_enabled;
        if ui
            .checkbox(&mut ai_on, "Enable AI (first-pass code suggestions)")
            .changed()
        {
            self.settings.ai_enabled = ai_on;
            if ai_on
                && self
                    .settings
                    .ai_provider_kind
                    .as_deref()
                    .unwrap_or("")
                    .is_empty()
            {
                self.settings.ai_provider_kind = Some("mock".into());
            }
            self.dual_write_ai_config();
        }
        let mut allow_remote = self.settings.ai_allow_remote;
        if ui
            .checkbox(
                &mut allow_remote,
                "Allow remote (cloud) providers — requires explicit enable",
            )
            .changed()
        {
            self.settings.ai_allow_remote = allow_remote;
            self.dual_write_ai_config();
        }
        ui.horizontal(|ui| {
            ui.label("Provider:");
            let mut kind = self
                .settings
                .ai_provider_kind
                .clone()
                .unwrap_or_else(|| "none".into());
            for (label, val) in [
                ("none", "none"),
                ("mock", "mock"),
                ("openai_compatible", "openai_compatible"),
            ] {
                if ui
                    .selectable_label(kind == val, label)
                    .on_hover_text(val)
                    .clicked()
                {
                    kind = val.into();
                }
            }
            if self.settings.ai_provider_kind.as_deref() != Some(kind.as_str()) {
                self.settings.ai_provider_kind = Some(kind);
                self.dual_write_ai_config();
            }
        });
        ui.horizontal(|ui| {
            ui.label("Base URL:");
            let mut url = self.settings.ai_base_url.clone().unwrap_or_default();
            let r = ui.add(
                egui::TextEdit::singleline(&mut url)
                    .desired_width(320.0)
                    .hint_text("http://127.0.0.1:11434/v1"),
            );
            if r.changed() || r.lost_focus() {
                self.settings.ai_base_url = if url.trim().is_empty() {
                    None
                } else {
                    Some(url.trim().to_string())
                };
                self.dual_write_ai_config();
            }
        });
        ui.horizontal(|ui| {
            ui.label("Model:");
            let mut model = self.settings.ai_model.clone().unwrap_or_default();
            let r = ui.add(
                egui::TextEdit::singleline(&mut model)
                    .desired_width(200.0)
                    .hint_text("llama3.2 or mock"),
            );
            if r.changed() || r.lost_focus() {
                self.settings.ai_model = if model.trim().is_empty() {
                    None
                } else {
                    Some(model.trim().to_string())
                };
                self.dual_write_ai_config();
            }
        });
        ui.horizontal(|ui| {
            ui.label("API key:");
            let r = ui.add(
                egui::TextEdit::singleline(&mut self.ai_api_key_draft)
                    .desired_width(220.0)
                    .password(true)
                    .hint_text("saved to OS keyring (not SQLite)"),
            );
            let save_clicked = ui
                .button("Save key")
                .on_hover_text("Store key in OS keyring only (never DeskSettings JSON)")
                .clicked();
            let enter_save = (r.lost_focus() || ui.input(|i| i.key_pressed(egui::Key::Enter)))
                && !self.ai_api_key_draft.trim().is_empty();
            if (save_clicked || enter_save) && !self.ai_api_key_draft.trim().is_empty() {
                match matter_ai::store_api_key(self.ai_api_key_draft.trim()) {
                    Ok(()) => {
                        self.ai_api_key_draft.clear();
                        self.status_msg = Some("API key stored in OS keyring.".into());
                        self.error_msg = None;
                    }
                    Err(e) => {
                        self.error_msg = Some(format!("Keyring store failed: {e}"));
                    }
                }
            }
            if ui
                .button("Clear key")
                .on_hover_text("Remove key from OS keyring")
                .clicked()
            {
                match matter_ai::delete_api_key() {
                    Ok(()) => {
                        self.ai_api_key_draft.clear();
                        self.status_msg = Some("API key cleared from keyring.".into());
                    }
                    Err(e) => self.error_msg = Some(format!("Keyring clear failed: {e}")),
                }
            }
        });
        ui.label(
            egui::RichText::new(
                "Headless: set env PST_DEDUPE_AI_API_KEY when keyring is unavailable. \
                 Local: Ollama :11434/v1 or LM Studio :1234/v1 (operator-installed).",
            )
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

        ui.add_space(8.0);
        ui.heading("Local STT (optional)");
        let stt_resp = ui.checkbox(
            &mut self.settings.stt_enabled,
            "Enable local STT (whisper.cpp CLI)",
        );
        if stt_resp.changed() {
            self.settings.save();
        }
        ui.label(
            egui::RichText::new(
                "Off by default. Offline only — no cloud STT. Requires operator-installed \
                 whisper.cpp + model weights (never downloaded automatically). Video needs ffmpeg \
                 (-ar 16000 -ac 1 -c:a pcm_s16le). Un-diarized transcripts may hallucinate; \
                 human must listen for attribution before treating as evidence. Not court reporting.",
            )
            .weak()
            .small(),
        );
        ui.horizontal(|ui| {
            ui.label("whisper-cli path:");
            let mut path = self.settings.whisper_cli_path.clone().unwrap_or_default();
            let r = ui.add(
                egui::TextEdit::singleline(&mut path)
                    .desired_width(280.0)
                    .hint_text("optional — uses PATH if empty"),
            );
            if r.changed() || r.lost_focus() {
                self.settings.whisper_cli_path = if path.trim().is_empty() {
                    None
                } else {
                    Some(path.trim().to_string())
                };
                self.settings.save();
            }
        });
        ui.horizontal(|ui| {
            ui.label("Model path:");
            let mut path = self.settings.stt_model_path.clone().unwrap_or_default();
            let r = ui.add(
                egui::TextEdit::singleline(&mut path)
                    .desired_width(280.0)
                    .hint_text("required — e.g. ggml-base.bin"),
            );
            if r.changed() || r.lost_focus() {
                self.settings.stt_model_path = if path.trim().is_empty() {
                    None
                } else {
                    Some(path.trim().to_string())
                };
                self.settings.save();
            }
        });
        ui.horizontal(|ui| {
            ui.label("ffmpeg path:");
            let mut path = self.settings.ffmpeg_path.clone().unwrap_or_default();
            let r = ui.add(
                egui::TextEdit::singleline(&mut path)
                    .desired_width(280.0)
                    .hint_text("optional — video / complex audio"),
            );
            if r.changed() || r.lost_focus() {
                self.settings.ffmpeg_path = if path.trim().is_empty() {
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
                Screen::Produce,
                Screen::Gap,
                Screen::People,
                Screen::Clusters,
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
                    if next == Screen::Gap {
                        if let Some(ref root) = self.matter_root {
                            self.gap.request_reload(root);
                        }
                    }
                    if next == Screen::People {
                        if let Some(ref root) = self.matter_root {
                            self.people.request_reload(root);
                        }
                    }
                    if next == Screen::Clusters {
                        if let Some(ref root) = self.matter_root {
                            self.clusters.request_reload(root);
                        }
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
        self.gap.poll();
        self.on_progress_tick();

        let snap = self.progress_rx.borrow().clone();
        progress_ui::request_job_repaint(&ctx, &snap);
        // Also repaint lightly while a dialog, matter op, overview load, or report export is in flight.
        if self.dialog.is_open()
            || self.matter_op.is_busy()
            || self.overview_load.is_busy()
            || self.report_export_busy
            || self.gap.busy
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
                    let mut semantic_req = None;
                    let index_job_busy = self.job_may_be_writing();
                    review_ui::show(
                        ui,
                        &mut self.review,
                        &root,
                        &actor,
                        &mut fts_req,
                        &mut semantic_req,
                        index_job_busy,
                    );
                    match fts_req {
                        Some(review_ui::FtsUiRequest::UpdateIndex) => self.start_fts_index(),
                        Some(review_ui::FtsUiRequest::RebuildIndex) => self.start_fts_rebuild(),
                        None => {}
                    }
                    match semantic_req {
                        Some(review_ui::SemanticUiRequest::UpdateIndex) => {
                            self.start_semantic_index()
                        }
                        Some(review_ui::SemanticUiRequest::RebuildIndex) => {
                            self.start_semantic_rebuild()
                        }
                        None => {}
                    }
                } else {
                    ui.label("Open a matter to review.");
                }
            }
            Screen::Gap => {
                let root = self.matter_root.clone();
                let busy = self.runner_busy();
                gap_ui::show(ui, &mut self.gap, root.as_deref(), busy);
                if gap_ui::take_start_collection(&mut self.gap) {
                    self.start_collection_gap();
                }
                if gap_ui::take_start_opposing(&mut self.gap) {
                    self.start_opposing_gap();
                }
            }
            Screen::People => {
                let root = self.matter_root.clone();
                let busy = self.runner_busy();
                people_ui::show(ui, &mut self.people, root.as_deref(), busy);
                if self.people.take_start() {
                    self.start_people_graph();
                }
                if let Some(pid) = self.people.take_filter_person() {
                    use matter_core::{FilterCondition, FilterSpec, SCOPE_ENTIRE_MATTER};
                    let mut spec = FilterSpec {
                        conditions: vec![FilterCondition {
                            field: "person_id".into(),
                            op: "eq".into(),
                            value: Some(serde_json::Value::String(pid)),
                            values: None,
                            start: None,
                            end: None,
                        }],
                        ..FilterSpec::default()
                    };
                    spec.scope = SCOPE_ENTIRE_MATTER.into();
                    if let Some(ref root) = self.matter_root {
                        self.review.apply_preset(root, spec);
                    }
                    self.screen = Screen::Review;
                    self.status_msg = Some("Filter applied: person_id".into());
                }
            }
            Screen::Clusters => {
                let root = self.matter_root.clone();
                let busy = self.runner_busy();
                cluster_ui::show(ui, &mut self.clusters, root.as_deref(), busy);
                if self.clusters.take_start() {
                    self.start_concept_cluster();
                }
                if let Some(cid) = self.clusters.take_filter_cluster() {
                    use matter_core::{FilterCondition, FilterSpec, SCOPE_ENTIRE_MATTER};
                    let mut spec = FilterSpec {
                        conditions: vec![FilterCondition {
                            field: "concept_cluster_id".into(),
                            op: "eq".into(),
                            value: Some(serde_json::Value::String(cid)),
                            values: None,
                            start: None,
                            end: None,
                        }],
                        ..FilterSpec::default()
                    };
                    spec.scope = SCOPE_ENTIRE_MATTER.into();
                    if let Some(ref root) = self.matter_root {
                        self.review.apply_preset(root, spec);
                    }
                    self.screen = Screen::Review;
                    self.status_msg = Some("Filter applied: concept_cluster_id".into());
                }
            }
            Screen::Produce => {
                ui.heading("Produce");
                ui.label(
                    "Export the review corpus as natives + text + Concordance DAT/CSV \
                     (track 0040). Run production QC first (track 0041).",
                );
                ui.add_space(8.0);
                // Always refresh soft-gate when Produce is shown (selection may have changed).
                if self.produce_qc_readiness_expand != self.produce_expand_family
                    || self.produce_qc_readiness_require != self.produce_require_qc_pass
                    || matches!(self.produce_qc_readiness, ProduceQcReadiness::Unknown)
                {
                    self.refresh_produce_qc_readiness();
                }
                let busy = self.runner_busy();
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(!busy, egui::Button::new("Run production QC"))
                        .on_hover_text(
                            "Scan the review corpus for broken families, withheld items, \
                             missing natives/text, redaction gaps, and more. Writes \
                             exports/qc/ findings CSV.",
                        )
                        .clicked()
                    {
                        self.start_production_qc();
                    }
                    if ui
                        .add_enabled(!busy, egui::Button::new("Produce review set…"))
                        .on_hover_text(
                            "Withheld items skipped by default. Family expand off. \
                             Require QC pass is on by default.",
                        )
                        .clicked()
                    {
                        self.open_produce_dialog();
                    }
                });
                ui.add_space(6.0);
                // QC summary chips (session + freshness preflight)
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Last QC:").strong());
                    match self.last_qc_passed {
                        Some(true) => {
                            ui.colored_label(egui::Color32::from_rgb(40, 140, 70), "passed");
                        }
                        Some(false) => {
                            ui.colored_label(egui::Color32::from_rgb(180, 50, 50), "failed");
                        }
                        None => {
                            ui.label(egui::RichText::new("none").weak());
                        }
                    }
                    if let Some(e) = self.last_qc_error_count {
                        ui.label(format!("errors={e}"));
                    }
                    if let Some(w) = self.last_qc_warn_count {
                        ui.label(format!("warns={w}"));
                    }
                });
                // Freshness chip (Missing / Failed / Stale / Passed)
                if self.produce_require_qc_pass {
                    let (color, text) = match &self.produce_qc_readiness {
                        ProduceQcReadiness::Allowed => (
                            egui::Color32::from_rgb(40, 140, 70),
                            "Gate: Passed (fresh)".to_string(),
                        ),
                        ProduceQcReadiness::Missing => (
                            egui::Color32::from_rgb(180, 50, 50),
                            "Gate: Missing".to_string(),
                        ),
                        ProduceQcReadiness::Failed { .. } => (
                            egui::Color32::from_rgb(180, 50, 50),
                            "Gate: Failed".to_string(),
                        ),
                        ProduceQcReadiness::Stale { .. } => (
                            egui::Color32::from_rgb(180, 120, 40),
                            "Gate: Stale — Selection changed since last QC — re-run QC".to_string(),
                        ),
                        ProduceQcReadiness::Unknown => (
                            egui::Color32::from_rgb(120, 120, 120),
                            "Gate: unknown".to_string(),
                        ),
                        ProduceQcReadiness::Unavailable(msg) => (
                            egui::Color32::from_rgb(180, 120, 40),
                            format!("Gate: unavailable ({msg})"),
                        ),
                    };
                    ui.colored_label(color, text);
                    if !matches!(
                        self.produce_qc_readiness,
                        ProduceQcReadiness::Allowed | ProduceQcReadiness::Unknown
                    ) {
                        ui.colored_label(
                            egui::Color32::from_rgb(180, 120, 40),
                            self.produce_qc_readiness.label(),
                        );
                    }
                }
                if let Some(ref path) = self.last_qc_report_path {
                    ui.label(
                        egui::RichText::new(format!("Report: {path}"))
                            .weak()
                            .small(),
                    );
                }
                if let Some(ref st) = self.last_qc_status {
                    ui.label(egui::RichText::new(st.clone()).small());
                }
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            self.last_qc_report_path.is_some(),
                            egui::Button::new(if self.qc_findings_show {
                                "Hide findings"
                            } else {
                                "Show findings"
                            }),
                        )
                        .on_hover_text("Load findings.csv from the last QC report (capped)")
                        .clicked()
                    {
                        if self.qc_findings_show {
                            self.qc_findings_show = false;
                        } else if let Some(ref path) = self.last_qc_report_path.clone() {
                            self.load_qc_findings_from_report(path);
                            self.qc_findings_show = true;
                        }
                    }
                    let report_exists = self
                        .last_qc_report_path
                        .as_ref()
                        .is_some_and(|p| Utf8Path::new(p.as_str()).exists());
                    if ui
                        .add_enabled(report_exists, egui::Button::new("Open findings folder"))
                        .on_hover_text("Open the QC report directory in File Explorer")
                        .on_disabled_hover_text("No report folder on disk yet")
                        .clicked()
                    {
                        self.open_qc_findings_folder();
                    }
                    if !self.qc_findings.is_empty() {
                        ui.label(format!(
                            "{} finding(s){}",
                            self.qc_findings.len(),
                            if self.qc_findings.len() >= FINDINGS_DISPLAY_CAP {
                                " (capped)"
                            } else {
                                ""
                            }
                        ));
                    }
                });
                if let Some(ref err) = self.qc_findings_error {
                    ui.colored_label(egui::Color32::from_rgb(180, 50, 50), err);
                }
                if self.qc_findings_show && !self.qc_findings.is_empty() {
                    let mut jump_item: Option<String> = None;
                    egui::ScrollArea::vertical()
                        .max_height(160.0)
                        .show(ui, |ui| {
                            egui::Grid::new("qc_findings_grid")
                                .num_columns(4)
                                .striped(true)
                                .show(ui, |ui| {
                                    ui.label(egui::RichText::new("rule").strong().small());
                                    ui.label(egui::RichText::new("sev").strong().small());
                                    ui.label(egui::RichText::new("item").strong().small());
                                    ui.label(egui::RichText::new("message").strong().small());
                                    ui.end_row();
                                    for row in &self.qc_findings {
                                        ui.label(egui::RichText::new(&row.rule_id).small());
                                        ui.label(egui::RichText::new(&row.severity).small());
                                        if row.item_id.is_empty() {
                                            ui.label(egui::RichText::new("—").small());
                                        } else if ui
                                            .add(
                                                egui::Button::new(
                                                    egui::RichText::new(&row.item_id).small(),
                                                )
                                                .frame(false),
                                            )
                                            .on_hover_text("Open in Review")
                                            .clicked()
                                        {
                                            jump_item = Some(row.item_id.clone());
                                        }
                                        ui.label(egui::RichText::new(&row.message).small());
                                        ui.end_row();
                                    }
                                });
                        });
                    if let Some(id) = jump_item {
                        self.open_review_item(&id);
                    }
                }
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(
                        "Default output: <matter>/exports/productions/<name>/\n\
                         DAT: UTF-8 BOM, þ/¶ delimiters, ® newlines, UTC dates.\n\
                         Privilege descriptions and notes are never included.\n\
                         QC expand_family must match produce expand (same checkbox).",
                    )
                    .weak()
                    .small(),
                );
            }
        });

        if self.produce_dialog_open {
            // Keep preflight aligned with checkboxes while dialog is open.
            if self.produce_qc_readiness_expand != self.produce_expand_family
                || self.produce_qc_readiness_require != self.produce_require_qc_pass
            {
                self.refresh_produce_qc_readiness();
            }
            egui::Window::new("Produce review set")
                .collapsible(false)
                .resizable(true)
                .default_width(420.0)
                .show(&ctx, |ui| {
                    ui.label("Packages in_review items (or fails if the corpus is empty).");
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.label("Name:");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.produce_name)
                                .desired_width(260.0),
                        );
                    });
                    ui.horizontal(|ui| {
                        ui.label("Bates prefix:");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.produce_bates_prefix)
                                .desired_width(120.0),
                        );
                    });
                    ui.checkbox(
                        &mut self.produce_fail_if_withheld,
                        "Fail if any selected item is withheld",
                    );
                    ui.checkbox(
                        &mut self.produce_expand_family,
                        "Expand families (include parents/children)",
                    );
                    if !self.produce_expand_family {
                        ui.colored_label(
                            egui::Color32::from_rgb(180, 120, 40),
                            "Family expand is OFF — ensure review membership is family-complete \
                             or run production QC for orphan/broken-family findings.",
                        );
                    }
                    ui.checkbox(
                        &mut self.produce_require_qc_pass,
                        "Require QC pass (fresh selection fingerprint)",
                    );
                    if self.produce_require_qc_pass {
                        match &self.produce_qc_readiness {
                            ProduceQcReadiness::Allowed => {
                                ui.colored_label(
                                    egui::Color32::from_rgb(40, 140, 70),
                                    "QC fresh pass — produce allowed.",
                                );
                            }
                            ProduceQcReadiness::Missing => {
                                ui.colored_label(
                                    egui::Color32::from_rgb(180, 50, 50),
                                    "No QC run yet — run production QC before produce.",
                                );
                            }
                            ProduceQcReadiness::Failed { .. } => {
                                ui.colored_label(
                                    egui::Color32::from_rgb(180, 50, 50),
                                    "Last QC failed — fix errors and re-run QC before produce.",
                                );
                            }
                            ProduceQcReadiness::Stale { .. } => {
                                ui.colored_label(
                                    egui::Color32::from_rgb(180, 120, 40),
                                    "Selection changed since last QC — re-run QC",
                                );
                            }
                            ProduceQcReadiness::Unknown => {
                                ui.colored_label(
                                    egui::Color32::from_rgb(180, 120, 40),
                                    "QC status unknown — refresh or run QC.",
                                );
                            }
                            ProduceQcReadiness::Unavailable(msg) => {
                                ui.colored_label(
                                    egui::Color32::from_rgb(180, 50, 50),
                                    format!("QC preflight failed: {msg}"),
                                );
                            }
                        }
                    }
                    ui.horizontal(|ui| {
                        ui.label("Output folder:");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.produce_output_dir)
                                .desired_width(240.0)
                                .hint_text("default: exports/productions/<name>/"),
                        );
                    });
                    ui.label(
                        egui::RichText::new(
                            "Leave output empty to write under the matter exports/productions/ tree.",
                        )
                        .weak()
                        .small(),
                    );
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        let busy = self.runner_busy();
                        // Soft-gate: require fresh pass (not merely last_qc_passed session flag).
                        let gate_blocks = self.produce_require_qc_pass
                            && !self.produce_qc_readiness.allows_produce();
                        let can_start = !busy && !gate_blocks;
                        let hover = if busy {
                            "A job is running.".to_string()
                        } else if gate_blocks {
                            self.produce_qc_readiness.label()
                        } else {
                            "Start packaging the review corpus.".into()
                        };
                        let start = ui
                            .add_enabled(can_start, egui::Button::new("Start produce"))
                            .on_disabled_hover_text(hover);
                        if start.clicked() {
                            self.start_produce();
                        }
                        if ui.button("Cancel").clicked() {
                            self.produce_dialog_open = false;
                        }
                    });
                });
        }

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

/// Open a directory in the OS file manager (Windows Explorer).
fn open_folder_in_explorer(path: &str) -> Result<(), String> {
    // Prefer explorer on Windows; fall back to `cmd /c start` for the folder path.
    #[cfg(windows)]
    {
        use std::process::Command;
        match Command::new("explorer").arg(path).spawn() {
            Ok(_) => Ok(()),
            Err(e) => {
                // Fallback: `start` treats first quoted arg as window title.
                Command::new("cmd")
                    .args(["/C", "start", "", path])
                    .spawn()
                    .map(|_| ())
                    .map_err(|e2| format!("explorer: {e}; start: {e2}"))
            }
        }
    }
    #[cfg(not(windows))]
    {
        use std::process::Command;
        Command::new("xdg-open")
            .arg(path)
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}
