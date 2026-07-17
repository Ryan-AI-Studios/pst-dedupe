//! Single matter-worker-thread process runner.

use std::collections::HashMap;
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
    pub fn active_job(&self) -> Option<JobSnapshot> {
        let guard = self.active.lock().expect("active lock");
        guard.as_ref().map(|a| JobSnapshot {
            job_id: a.job_id.clone(),
            kind: a.kind.clone(),
            matter_id: a.matter_id.clone(),
            matter_root: a.matter_root.clone(),
            state: "running".into(),
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
            // Join without a hard timeout; stages pause cooperatively.
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
                        match run_start(
                            &matter,
                            handler.as_ref(),
                            &kind,
                            &params_json,
                            &matter_root,
                            &active,
                            &sink,
                            &reply,
                        ) {
                            Ok(()) => {}
                            Err(e) => {
                                // reply may already have been sent with job_id
                                let _ = e;
                            }
                        }
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
                        match run_resume(
                            &matter,
                            &job_id,
                            &matter_root,
                            &handlers,
                            &active,
                            &sink,
                            &reply,
                        ) {
                            Ok(()) => {}
                            Err(e) => {
                                let _ = e;
                            }
                        }
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
    let job = matter.create_job(kind)?;
    matter.set_job_state(&job.id, JobState::Running, None)?;

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
    let job = matter
        .get_job(job_id)
        .map_err(|_| RunnerError::JobNotFound(job_id.to_string()))?;

    let handler = {
        let map = handlers.lock().expect("handlers");
        map.get(&job.kind).cloned()
    };
    let Some(handler) = handler else {
        let _ = reply.send(Err(RunnerError::UnknownKind(job.kind.clone())));
        return Err(RunnerError::UnknownKind(job.kind));
    };

    match job.state {
        JobState::Paused | JobState::Failed | JobState::Pending | JobState::Running => {}
        JobState::Succeeded => {
            let _ = reply.send(Err(RunnerError::InvalidJob(format!(
                "{job_id}: already succeeded"
            ))));
            return Err(RunnerError::InvalidJob(format!(
                "{job_id}: already succeeded"
            )));
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
            let _ = matter.set_job_state(&job.id, JobState::Pending, None);
        }
        let _ = matter.set_job_state(&job.id, JobState::Running, None);
    }

    publish_started(sink, &job, &job.kind, matter.id());
    let _ = reply.send(Ok(()));

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
    finalize_job(matter, &job.id, &job.kind, matter.id(), sink, outcome);

    *active.lock().expect("active") = None;
    Ok(())
}

fn load_resume_params(matter: &Matter, job: &Job) -> String {
    // Prefer job-kind-specific checkpoint cursor for source_id when present.
    let stages = ["expand", "pst_extract"];
    for stage in stages {
        if let Ok(Some(cp)) = matter.get_checkpoint(&job.id, stage) {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&cp.cursor_json) {
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
