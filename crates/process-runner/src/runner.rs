//! Single matter-worker-thread process runner.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use camino::{Utf8Path, Utf8PathBuf};
use matter_core::{Job, JobState, Matter};
use tokio::sync::{broadcast, watch};

use crate::cancel::CancelToken;
use crate::config::RunnerConfig;
use crate::error::{Result, RunnerError};
use crate::handler::{JobContext, JobHandler, JobOutcome, JobParams};
use crate::progress::{now_rfc3339_approx, JobProgressSnapshot, ProgressEvent, ProgressSink};

/// Stages the runner polls for mid-run `completed_count` (DoD-4).
const PROGRESS_STAGES: &[&str] = &[
    "expand",
    "pst_extract",
    "dedupe",
    "thread",
    "neardup",
    "cull",
    "promote",
    "produce",
    "qc",
    "gap",
    "fts",
    "office",
    "pdf",
    "ics",
    "ocr",
    "classify",
    "profile_run",
    "workflow_run",
];

/// Clone an error for channel delivery (Matter errors become `Other` text).
fn clone_for_reply(err: &RunnerError) -> RunnerError {
    match err {
        RunnerError::Busy { job_id } => RunnerError::Busy {
            job_id: job_id.clone(),
        },
        RunnerError::UnknownKind(k) => RunnerError::UnknownKind(k.clone()),
        RunnerError::HandlerFailed(m) => RunnerError::HandlerFailed(m.clone()),
        RunnerError::MatterOpen { path, message } => RunnerError::MatterOpen {
            path: path.clone(),
            message: message.clone(),
        },
        RunnerError::JobNotFound(id) => RunnerError::JobNotFound(id.clone()),
        RunnerError::InvalidJob(m) => RunnerError::InvalidJob(m.clone()),
        RunnerError::InvalidParams(m) => RunnerError::InvalidParams(m.clone()),
        RunnerError::CancelFailed(m) => RunnerError::CancelFailed(m.clone()),
        RunnerError::ShutDown => RunnerError::ShutDown,
        RunnerError::WorkerGone => RunnerError::WorkerGone,
        RunnerError::Matter(e) => RunnerError::Other(e.to_string()),
        RunnerError::Other(m) => RunnerError::Other(m.clone()),
    }
}

fn reply_err_string(reply: &Sender<Result<String>>, err: RunnerError) -> RunnerError {
    let _ = reply.send(Err(clone_for_reply(&err)));
    err
}

fn reply_err_unit(reply: &Sender<Result<()>>, err: RunnerError) -> RunnerError {
    let _ = reply.send(Err(clone_for_reply(&err)));
    err
}

/// Lightweight view of the currently active job (if any).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobSnapshot {
    pub job_id: String,
    pub kind: String,
    pub matter_id: String,
    pub matter_root: String,
    pub state: String,
}

struct ActiveJob {
    job_id: String,
    kind: String,
    matter_id: String,
    matter_root: String,
    cancel: CancelToken,
}

enum Command {
    Start {
        matter_root: Utf8PathBuf,
        kind: String,
        params_json: String,
        reply: Sender<Result<String>>,
    },
    Resume {
        matter_root: Utf8PathBuf,
        job_id: String,
        reply: Sender<Result<()>>,
    },
    Cancel {
        job_id: String,
        reply: Sender<Result<()>>,
    },
    Shutdown,
}

/// In-process job runner: **one** matter worker thread owns `Matter` for the
/// active job. Never share `Matter` across rayon under a mutex for P0.
pub struct ProcessRunner {
    cmd_tx: Mutex<Option<Sender<Command>>>,
    join: Mutex<Option<JoinHandle<()>>>,
    progress_rx: watch::Receiver<JobProgressSnapshot>,
    event_tx: Option<broadcast::Sender<ProgressEvent>>,
    active: Arc<Mutex<Option<ActiveJob>>>,
    handlers: Arc<Mutex<HashMap<String, Arc<dyn JobHandler>>>>,
    /// Serializes start/resume accept so a second start sees `active` busy
    /// instead of queueing behind a still-running job.
    accept: Mutex<()>,
}

impl ProcessRunner {
    /// Create a runner and start the single matter worker thread.
    ///
    /// Register handlers with [`ProcessRunner::register`] before calling
    /// [`ProcessRunner::start`].
    pub fn new(config: RunnerConfig) -> Self {
        let (progress_tx, progress_rx) = watch::channel(JobProgressSnapshot::idle());
        let event_tx = if config.enable_broadcast {
            let (tx, _) = broadcast::channel(config.broadcast_capacity.max(1));
            Some(tx)
        } else {
            None
        };

        let (cmd_tx, cmd_rx) = mpsc::channel::<Command>();
        let active = Arc::new(Mutex::new(None));
        let handlers = Arc::new(Mutex::new(HashMap::new()));

        let worker_active = Arc::clone(&active);
        let worker_handlers = Arc::clone(&handlers);
        let worker_progress = progress_tx.clone();
        let worker_events = event_tx.clone();

        let join = thread::Builder::new()
            .name("matter-worker".into())
            .spawn(move || {
                worker_loop(
                    cmd_rx,
                    worker_handlers,
                    worker_active,
                    worker_progress,
                    worker_events,
                );
            })
            .expect("spawn matter worker");

        // Keep only the receiver; the worker holds a cloned Sender.
        drop(progress_tx);
        Self {
            cmd_tx: Mutex::new(Some(cmd_tx)),
            join: Mutex::new(Some(join)),
            progress_rx,
            event_tx,
            active,
            handlers,
            accept: Mutex::new(()),
        }
    }

    /// Register a handler for its [`JobHandler::kind`]. Replaces any existing.
    pub fn register(&mut self, handler: Arc<dyn JobHandler>) {
        let kind = handler.kind().to_string();
        self.handlers
            .lock()
            .expect("handlers lock")
            .insert(kind, handler);
    }

    /// Create a job, set Running, and queue work on the matter worker.
    ///
    /// Returns `job_id` once the job is created and accepted (before the
    /// handler finishes). Progress is available via [`Self::watch_progress`].
    ///
    /// Rejects with [`RunnerError::Busy`] if a job is already active.
    pub fn start(&self, matter_root: &Utf8Path, kind: &str, params: JobParams) -> Result<String> {
        let _accept = self.accept.lock().expect("accept lock");
        if let Some(ref a) = *self.active.lock().expect("active lock") {
            return Err(RunnerError::Busy {
                job_id: a.job_id.clone(),
            });
        }
        let (reply_tx, reply_rx) = mpsc::channel();
        self.send(Command::Start {
            matter_root: matter_root.to_path_buf(),
            kind: kind.to_string(),
            params_json: params.json,
            reply: reply_tx,
        })?;
        reply_rx.recv().map_err(|_| RunnerError::WorkerGone)?
    }

    /// Resume a paused/failed job on the matter worker.
    ///
    /// Returns once the resume is **accepted** (job found, handler known);
    /// completion is observed via [`Self::watch_progress`].
    pub fn resume(&self, matter_root: &Utf8Path, job_id: &str) -> Result<()> {
        let _accept = self.accept.lock().expect("accept lock");
        if let Some(ref a) = *self.active.lock().expect("active lock") {
            return Err(RunnerError::Busy {
                job_id: a.job_id.clone(),
            });
        }
        let (reply_tx, reply_rx) = mpsc::channel();
        self.send(Command::Resume {
            matter_root: matter_root.to_path_buf(),
            job_id: job_id.to_string(),
            reply: reply_tx,
        })?;
        reply_rx.recv().map_err(|_| RunnerError::WorkerGone)?
    }

    /// Request cooperative cancel for the active job (must match `job_id`).
    pub fn cancel(&self, job_id: &str) -> Result<()> {
        // Fast path: set cancel flag without waiting for the worker queue.
        {
            let guard = self.active.lock().expect("active lock");
            if let Some(ref a) = *guard {
                if a.job_id == job_id {
                    a.cancel.cancel();
                    return Ok(());
                }
            }
        }
        // Fallback via worker (job may not yet be marked active).
        let (reply_tx, reply_rx) = mpsc::channel();
        self.send(Command::Cancel {
            job_id: job_id.to_string(),
            reply: reply_tx,
        })?;
        reply_rx.recv().map_err(|_| RunnerError::WorkerGone)?
    }

    /// Subscribe to the latest progress snapshot (clone of the watch receiver).
    pub fn watch_progress(&self) -> watch::Receiver<JobProgressSnapshot> {
        self.progress_rx.clone()
    }

    /// Optional full event stream. Returns `None` if broadcast is disabled.
    pub fn subscribe_events(&self) -> Option<broadcast::Receiver<ProgressEvent>> {
        self.event_tx.as_ref().map(|tx| tx.subscribe())
    }

    /// Snapshot of the currently active job, if any.
    ///
    /// When `matter_id` is `Some`, only returns the active job if it belongs
    /// to that matter (P0 runner is single-flight globally; filter matches
    /// the spec sketch `active_job(matter_id)`).
    pub fn active_job(&self, matter_id: Option<&str>) -> Option<JobSnapshot> {
        let guard = self.active.lock().expect("active lock");
        guard.as_ref().and_then(|a| {
            if let Some(want) = matter_id {
                if a.matter_id != want {
                    return None;
                }
            }
            Some(JobSnapshot {
                job_id: a.job_id.clone(),
                kind: a.kind.clone(),
                matter_id: a.matter_id.clone(),
                matter_root: a.matter_root.clone(),
                state: "running".into(),
            })
        })
    }

    /// Whether the worker currently holds an active job.
    pub fn is_busy(&self) -> bool {
        self.active.lock().expect("active lock").is_some()
    }

    /// Cancel any active job and join the worker thread.
    pub fn shutdown(&self) {
        // Cancel active work first.
        if let Some(ref a) = *self.active.lock().expect("active lock") {
            a.cancel.cancel();
        }
        // Drop sender after Shutdown so the worker can exit.
        let tx = self.cmd_tx.lock().expect("cmd lock").take();
        if let Some(tx) = tx {
            let _ = tx.send(Command::Shutdown);
            // Dropping tx closes the channel after Shutdown is sent.
            drop(tx);
        }
        if let Some(handle) = self.join.lock().expect("join lock").take() {
            // Join without a wall-clock timeout (documented in README). Stages
            // must poll cancel so Drop does not hang indefinitely.
            let _ = handle.join();
        }
    }

    /// Block until the runner is idle (no active job) or `timeout` elapses.
    pub fn wait_until_idle(&self, timeout: Duration) -> bool {
        let start = std::time::Instant::now();
        while start.elapsed() < timeout {
            if !self.is_busy() {
                return true;
            }
            thread::sleep(Duration::from_millis(10));
        }
        !self.is_busy()
    }

    fn send(&self, cmd: Command) -> Result<()> {
        let guard = self.cmd_tx.lock().expect("cmd lock");
        let tx = guard.as_ref().ok_or(RunnerError::ShutDown)?;
        tx.send(cmd).map_err(|_| RunnerError::WorkerGone)
    }
}

impl Drop for ProcessRunner {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn worker_loop(
    cmd_rx: Receiver<Command>,
    handlers: Arc<Mutex<HashMap<String, Arc<dyn JobHandler>>>>,
    active: Arc<Mutex<Option<ActiveJob>>>,
    progress_tx: watch::Sender<JobProgressSnapshot>,
    event_tx: Option<broadcast::Sender<ProgressEvent>>,
) {
    let sink = ProgressSink::new(progress_tx, event_tx);

    while let Ok(cmd) = cmd_rx.recv() {
        match cmd {
            Command::Shutdown => {
                if let Some(ref a) = *active.lock().expect("active") {
                    a.cancel.cancel();
                }
                break;
            }
            Command::Cancel { job_id, reply } => {
                let res = cancel_active(&active, &job_id);
                let _ = reply.send(res);
            }
            Command::Start {
                matter_root,
                kind,
                params_json,
                reply,
            } => {
                // Single-flight: reject if already running.
                if let Some(ref a) = *active.lock().expect("active") {
                    let _ = reply.send(Err(RunnerError::Busy {
                        job_id: a.job_id.clone(),
                    }));
                    continue;
                }

                let handler = {
                    let map = handlers.lock().expect("handlers");
                    map.get(&kind).cloned()
                };
                let Some(handler) = handler else {
                    let _ = reply.send(Err(RunnerError::UnknownKind(kind)));
                    continue;
                };

                match open_matter(&matter_root) {
                    Ok(matter) => {
                        // run_start always replies (success with job_id, or typed Err).
                        let _ = run_start(
                            &matter,
                            handler.as_ref(),
                            &kind,
                            &params_json,
                            &matter_root,
                            &active,
                            &sink,
                            &reply,
                        );
                        // Matter drops here — exclusive ownership ends.
                    }
                    Err(e) => {
                        let _ = reply.send(Err(e));
                    }
                }
            }
            Command::Resume {
                matter_root,
                job_id,
                reply,
            } => {
                if let Some(ref a) = *active.lock().expect("active") {
                    let _ = reply.send(Err(RunnerError::Busy {
                        job_id: a.job_id.clone(),
                    }));
                    continue;
                }

                match open_matter(&matter_root) {
                    Ok(matter) => {
                        // run_resume always replies (Ok(()) or typed Err).
                        let _ = run_resume(
                            &matter,
                            &job_id,
                            &matter_root,
                            &handlers,
                            &active,
                            &sink,
                            &reply,
                        );
                    }
                    Err(e) => {
                        let _ = reply.send(Err(e));
                    }
                }
            }
        }
    }

    // Clear active on exit.
    *active.lock().expect("active") = None;
}

fn open_matter(root: &Utf8Path) -> Result<Matter> {
    Matter::open(root).map_err(|e| RunnerError::MatterOpen {
        path: root.to_string(),
        message: e.to_string(),
    })
}

fn cancel_active(active: &Arc<Mutex<Option<ActiveJob>>>, job_id: &str) -> Result<()> {
    let guard = active.lock().expect("active");
    match guard.as_ref() {
        Some(a) if a.job_id == job_id => {
            a.cancel.cancel();
            Ok(())
        }
        Some(a) => Err(RunnerError::CancelFailed(format!(
            "active job is {}, not {job_id}",
            a.job_id
        ))),
        None => Err(RunnerError::CancelFailed("no active job to cancel".into())),
    }
}

#[allow(clippy::too_many_arguments)]
fn run_start(
    matter: &Matter,
    handler: &dyn JobHandler,
    kind: &str,
    params_json: &str,
    matter_root: &Utf8Path,
    active: &Arc<Mutex<Option<ActiveJob>>>,
    sink: &ProgressSink,
    reply: &Sender<Result<String>>,
) -> Result<()> {
    // --- Accept phase: always reply with job_id or a typed error (never drop). ---
    // Durable single-flight: refuse if a prior process left a Running job row.
    if let Ok(jobs) = matter.list_jobs() {
        if let Some(running) = jobs.iter().find(|j| j.state == JobState::Running) {
            return Err(reply_err_string(
                reply,
                RunnerError::Busy {
                    job_id: running.id.clone(),
                },
            ));
        }
    }

    let job = match matter.create_job(kind) {
        Ok(j) => j,
        Err(e) => {
            return Err(reply_err_string(reply, RunnerError::Matter(e)));
        }
    };
    if let Err(e) = matter.set_job_state(&job.id, JobState::Running, None) {
        return Err(reply_err_string(reply, RunnerError::Matter(e)));
    }

    let cancel = CancelToken::new();
    {
        let mut guard = active.lock().expect("active");
        *guard = Some(ActiveJob {
            job_id: job.id.clone(),
            kind: kind.to_string(),
            matter_id: matter.id().to_string(),
            matter_root: matter_root.to_string(),
            cancel: cancel.clone(),
        });
    }

    publish_started(sink, &job, kind, matter.id());
    // Hand job_id back before long work so callers can cancel / watch.
    let _ = reply.send(Ok(job.id.clone()));

    // Mid-run progress: second SQLite connection (WAL) polls checkpoints.
    let poller = start_checkpoint_poller(matter_root, &job.id, kind, matter.id(), sink.clone());

    let ctx = JobContext {
        matter,
        job_id: &job.id,
        source_id: None,
        params_json,
        cancel: &cancel,
        progress: sink.clone(),
        is_resume: false,
    };

    let outcome = handler.run(&ctx);
    stop_checkpoint_poller(poller);
    finalize_job(matter, &job.id, kind, matter.id(), sink, outcome);

    *active.lock().expect("active") = None;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn run_resume(
    matter: &Matter,
    job_id: &str,
    matter_root: &Utf8Path,
    handlers: &Arc<Mutex<HashMap<String, Arc<dyn JobHandler>>>>,
    active: &Arc<Mutex<Option<ActiveJob>>>,
    sink: &ProgressSink,
    reply: &Sender<Result<()>>,
) -> Result<()> {
    // --- Accept phase: always reply Ok or typed Err (never drop → WorkerGone). ---
    let job = match matter.get_job(job_id) {
        Ok(j) => j,
        Err(_) => {
            return Err(reply_err_unit(
                reply,
                RunnerError::JobNotFound(job_id.to_string()),
            ));
        }
    };

    // Durable single-flight: another job row still Running blocks resume of a different job.
    if let Ok(jobs) = matter.list_jobs() {
        if let Some(running) = jobs
            .iter()
            .find(|j| j.state == JobState::Running && j.id != job_id)
        {
            return Err(reply_err_unit(
                reply,
                RunnerError::Busy {
                    job_id: running.id.clone(),
                },
            ));
        }
    }

    let handler = {
        let map = handlers.lock().expect("handlers");
        map.get(&job.kind).cloned()
    };
    let Some(handler) = handler else {
        return Err(reply_err_unit(
            reply,
            RunnerError::UnknownKind(job.kind.clone()),
        ));
    };

    match job.state {
        JobState::Paused | JobState::Failed | JobState::Pending | JobState::Running => {}
        JobState::Succeeded => {
            return Err(reply_err_unit(
                reply,
                RunnerError::InvalidJob(format!("{job_id}: already succeeded")),
            ));
        }
        JobState::Cancelled => {
            // Allowed: stages transition Cancelled → Pending → Running.
        }
    }

    // Prefer source_id from checkpoint params when present; resume handlers
    // re-load from checkpoints. Params for resume carry source_id when needed.
    let params_json = load_resume_params(matter, &job);

    let cancel = CancelToken::new();
    {
        let mut guard = active.lock().expect("active");
        *guard = Some(ActiveJob {
            job_id: job.id.clone(),
            kind: job.kind.clone(),
            matter_id: matter.id().to_string(),
            matter_root: matter_root.to_string(),
            cancel: cancel.clone(),
        });
    }

    // Bring durable state to Running before the handler (Paused/Failed/Pending).
    // Stages may also transition; same-state set is a no-op.
    if matches!(
        job.state,
        JobState::Paused | JobState::Failed | JobState::Pending | JobState::Cancelled
    ) {
        if job.state == JobState::Cancelled {
            if let Err(e) = matter.set_job_state(&job.id, JobState::Pending, None) {
                *active.lock().expect("active") = None;
                return Err(reply_err_unit(reply, RunnerError::Matter(e)));
            }
        }
        if let Err(e) = matter.set_job_state(&job.id, JobState::Running, None) {
            *active.lock().expect("active") = None;
            return Err(reply_err_unit(reply, RunnerError::Matter(e)));
        }
    }

    publish_started(sink, &job, &job.kind, matter.id());
    let _ = reply.send(Ok(()));

    let poller =
        start_checkpoint_poller(matter_root, &job.id, &job.kind, matter.id(), sink.clone());

    let source_id = extract_source_id(&params_json);
    let source_ref = source_id.as_deref();

    let ctx = JobContext {
        matter,
        job_id: &job.id,
        source_id: source_ref,
        params_json: &params_json,
        cancel: &cancel,
        progress: sink.clone(),
        is_resume: true,
    };

    let outcome = handler.run(&ctx);
    stop_checkpoint_poller(poller);
    finalize_job(matter, &job.id, &job.kind, matter.id(), sink, outcome);

    *active.lock().expect("active") = None;
    Ok(())
}

/// Companion thread: opens a **second** Matter connection (WAL) and mirrors
/// checkpoint `completed_count` into the watch sink while the handler blocks.
struct CheckpointPoller {
    stop: Arc<AtomicBool>,
    handle: JoinHandle<()>,
}

fn start_checkpoint_poller(
    matter_root: &Utf8Path,
    job_id: &str,
    kind: &str,
    matter_id: &str,
    sink: ProgressSink,
) -> CheckpointPoller {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_flag = Arc::clone(&stop);
    let root = matter_root.to_path_buf();
    let job_id = job_id.to_string();
    let kind = kind.to_string();
    let matter_id = matter_id.to_string();

    let handle = thread::Builder::new()
        .name("progress-poller".into())
        .spawn(move || {
            // Brief settle so the worker's first writes are visible.
            thread::sleep(Duration::from_millis(20));
            while !stop_flag.load(Ordering::SeqCst) {
                // open_for_read: never cleanup workspace/temp (CAS PST materialize race).
                if let Ok(m) = Matter::open_for_read(&root) {
                    for stage in PROGRESS_STAGES {
                        if let Ok(Some(cp)) = m.get_checkpoint(&job_id, stage) {
                            let count = cp.completed_count.max(0) as u64;
                            sink.patch(|s| {
                                // Only update if this is still the same job.
                                if s.job_id == job_id && s.state == "running" {
                                    s.completed_count = count;
                                    // Orchestration parents own stage/message labels
                                    // (current node / stage). Only mirror count.
                                    if *stage != "workflow_run" && *stage != "profile_run" {
                                        s.stage = Some((*stage).to_string());
                                        s.message = Some(format!("checkpoint:{stage}"));
                                    }
                                    s.kind = kind.clone();
                                    s.matter_id = matter_id.clone();
                                }
                            });
                        }
                    }
                }
                thread::sleep(Duration::from_millis(50));
            }
        })
        .expect("spawn progress poller");

    CheckpointPoller { stop, handle }
}

fn stop_checkpoint_poller(poller: CheckpointPoller) {
    poller.stop.store(true, Ordering::SeqCst);
    let _ = poller.handle.join();
}

fn load_resume_params(matter: &Matter, job: &Job) -> String {
    // Restore full frozen params from checkpoint when present (dedupe stores
    // use_message_id / family_policy / batch_size under cursor.params). Fall
    // back to source_id-only for expand/extract cursors.
    // Prefer the checkpoint stage matching this job kind (qc/produce/etc.),
    // then fall back to the full stage list so resume restores frozen params.
    let kind_stage = match job.kind.as_str() {
        "qc" => Some("qc"),
        "gap" => Some("gap"),
        "produce" | "production_export" => Some("produce"),
        "dedupe" => Some("dedupe"),
        "thread" => Some("thread"),
        "neardup" => Some("neardup"),
        "cull" => Some("cull"),
        "promote" => Some("promote"),
        "fts_index" | "fts" => Some("fts"),
        "profile_run" => Some("profile_run"),
        "workflow_run" => Some("workflow_run"),
        other => Some(other),
    };
    let mut stages: Vec<&str> = Vec::new();
    if let Some(s) = kind_stage {
        stages.push(s);
    }
    for s in PROGRESS_STAGES {
        if !stages.contains(s) {
            stages.push(s);
        }
    }
    for stage in stages {
        if let Ok(Some(cp)) = matter.get_checkpoint(&job.id, stage) {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&cp.cursor_json) {
                if let Some(params) = v.get("params") {
                    if params.as_object().is_some_and(|o| !o.is_empty()) {
                        return params.to_string();
                    }
                }
                // Top-level looks like dedupe / thread params (legacy / flat cursor).
                if v.get("use_message_id").is_some()
                    || v.get("family_policy").is_some()
                    || v.get("use_headers").is_some()
                    || v.get("use_subject_fallback").is_some()
                {
                    return v.to_string();
                }
                // QC/produce cursors may only carry nested params; empty object → keep looking.
                if let Some(sid) = v.get("source_id").and_then(|x| x.as_str()) {
                    return serde_json::json!({ "source_id": sid }).to_string();
                }
            }
        }
    }
    "{}".into()
}

fn extract_source_id(params_json: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(params_json)
        .ok()
        .and_then(|v| {
            v.get("source_id")
                .and_then(|x| x.as_str())
                .map(|s| s.to_string())
        })
}

fn publish_started(sink: &ProgressSink, job: &Job, kind: &str, matter_id: &str) {
    sink.update(JobProgressSnapshot {
        job_id: job.id.clone(),
        kind: kind.to_string(),
        matter_id: matter_id.to_string(),
        state: JobState::Running.as_str().into(),
        stage: None,
        completed_count: 0,
        total_hint: None,
        message: Some("started".into()),
        error_summary: None,
        updated_at: now_rfc3339_approx(),
    });
}

fn finalize_job(
    matter: &Matter,
    job_id: &str,
    kind: &str,
    matter_id: &str,
    sink: &ProgressSink,
    outcome: std::result::Result<JobOutcome, RunnerError>,
) {
    // Prefer durable job state written by the stage; fall back to outcome mapping.
    let durable = matter.get_job(job_id).ok();
    let (state, message, completed, error_summary) = match (&outcome, durable.as_ref()) {
        (_, Some(j)) if j.state != JobState::Running => {
            let (msg, count) = match &outcome {
                Ok(JobOutcome::Succeeded {
                    message,
                    completed_count,
                }) => (message.clone(), *completed_count),
                Ok(JobOutcome::Paused {
                    message,
                    completed_count,
                }) => (message.clone(), *completed_count),
                Ok(JobOutcome::Failed { message }) => (Some(message.clone()), 0),
                Err(e) => (Some(e.to_string()), 0),
            };
            (j.state, msg, count, j.error_summary.clone())
        }
        (
            Ok(JobOutcome::Succeeded {
                message,
                completed_count,
            }),
            _,
        ) => {
            let _ = matter.set_job_state(job_id, JobState::Succeeded, None);
            (JobState::Succeeded, message.clone(), *completed_count, None)
        }
        (
            Ok(JobOutcome::Paused {
                message,
                completed_count,
            }),
            _,
        ) => {
            let summary = message.as_deref().unwrap_or("paused");
            let _ = matter.set_job_state(job_id, JobState::Paused, Some(summary));
            (
                JobState::Paused,
                message.clone(),
                *completed_count,
                Some(summary.to_string()),
            )
        }
        (Ok(JobOutcome::Failed { message }), _) => {
            let _ = matter.set_job_state(job_id, JobState::Failed, Some(message));
            (
                JobState::Failed,
                Some(message.clone()),
                0,
                Some(message.clone()),
            )
        }
        (Err(e), _) => {
            let msg = e.to_string();
            let _ = matter.set_job_state(job_id, JobState::Failed, Some(&msg));
            (JobState::Failed, Some(msg.clone()), 0, Some(msg))
        }
    };

    sink.update(JobProgressSnapshot {
        job_id: job_id.to_string(),
        kind: kind.to_string(),
        matter_id: matter_id.to_string(),
        state: state.as_str().into(),
        stage: None,
        completed_count: completed,
        total_hint: None,
        message,
        error_summary,
        updated_at: now_rfc3339_approx(),
    });
}
