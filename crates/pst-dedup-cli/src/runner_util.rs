//! ProcessRunner bootstrap, wait loop, and SIGINT cancel (track 0045 §3.5).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use camino::Utf8Path;
use matter_core::{Job, JobState, Matter};
use process_runner::{
    register_default_handlers, JobParams, JobProgressSnapshot, ProcessRunner, RunnerConfig,
};
use serde_json::json;

use crate::error::{CliError, Result};
use crate::json_io::{emit_json, ok_envelope};

/// Kinds allowed for `job run` — shared with process-runner default registration.
pub fn allowed_job_kinds() -> &'static [&'static str] {
    process_runner::default_handler_kinds()
}

/// Shared cancel requested flag for SIGINT (handler only sets flags — no process::exit).
static SIGINT_COUNT: AtomicBool = AtomicBool::new(false);
static SIGINT_FORCE: AtomicBool = AtomicBool::new(false);

/// Build a runner with the shared default handler set.
pub fn build_runner() -> ProcessRunner {
    let mut runner = ProcessRunner::new(RunnerConfig::default());
    register_default_handlers(&mut runner);
    runner
}

/// Install Ctrl+C handler: first press sets cancel; second sets force flag.
///
/// Never calls `process::exit`. Runner cancel is applied in the wait loop.
/// Returns an error if the handler cannot be installed (do not start the job).
pub fn install_sigint_handler(cancel_job_id: Arc<std::sync::Mutex<Option<String>>>) -> Result<()> {
    ctrlc::set_handler(move || {
        if SIGINT_COUNT.swap(true, Ordering::SeqCst) {
            // Second Ctrl+C: force flag (documented last resort).
            SIGINT_FORCE.store(true, Ordering::SeqCst);
            eprintln!("signal: second Ctrl+C — force abort requested after cancel");
        } else {
            eprintln!("signal: Ctrl+C — requesting job cancel (waiting for graceful shutdown)");
            if let Ok(guard) = cancel_job_id.lock() {
                if let Some(ref id) = *guard {
                    // Best-effort: flag only; wait loop also calls runner.cancel.
                    let _ = id;
                }
            }
        }
    })
    .map_err(|e| {
        CliError::Msg(format!(
            "failed to install SIGINT handler (refusing to start job): {e}"
        ))
    })
}

/// Run a job to terminal state; progress → stderr only.
pub fn run_job_wait(
    matter_root: &Utf8Path,
    kind: &str,
    params_json: &str,
    json: bool,
) -> Result<Job> {
    if !allowed_job_kinds().contains(&kind) {
        return Err(CliError::Usage(format!(
            "unknown job kind '{kind}'; allowed: {}",
            allowed_job_kinds().join(", ")
        )));
    }

    let runner = build_runner();
    let cancel_slot: Arc<std::sync::Mutex<Option<String>>> = Arc::new(std::sync::Mutex::new(None));
    install_sigint_handler(Arc::clone(&cancel_slot))?;
    SIGINT_COUNT.store(false, Ordering::SeqCst);
    SIGINT_FORCE.store(false, Ordering::SeqCst);

    let mut progress = runner.watch_progress();
    let job_id = runner
        .start(matter_root, kind, JobParams::new(params_json))
        .map_err(CliError::from)?;

    if let Ok(mut g) = cancel_slot.lock() {
        *g = Some(job_id.clone());
    }

    wait_for_terminal(&runner, &mut progress, &job_id)?;
    let completed_count = progress.borrow().completed_count;

    // Graceful join before reopening SQLite.
    runner.shutdown();

    let matter = Matter::open_for_read(matter_root).map_err(CliError::from)?;
    let job = matter.get_job(&job_id).map_err(CliError::from)?;
    let sigint = SIGINT_COUNT.load(Ordering::SeqCst);
    emit_job_result(json, &job, sigint, completed_count)?;
    job_to_result(job, sigint)
}

/// Resume a job to terminal.
pub fn resume_job_wait(matter_root: &Utf8Path, job_id: &str, json: bool) -> Result<Job> {
    let runner = build_runner();
    let cancel_slot: Arc<std::sync::Mutex<Option<String>>> =
        Arc::new(std::sync::Mutex::new(Some(job_id.to_string())));
    install_sigint_handler(Arc::clone(&cancel_slot))?;
    SIGINT_COUNT.store(false, Ordering::SeqCst);
    SIGINT_FORCE.store(false, Ordering::SeqCst);

    let mut progress = runner.watch_progress();
    runner.resume(matter_root, job_id).map_err(CliError::from)?;

    wait_for_terminal(&runner, &mut progress, job_id)?;
    let completed_count = progress.borrow().completed_count;
    runner.shutdown();

    let matter = Matter::open_for_read(matter_root).map_err(CliError::from)?;
    let job = matter.get_job(job_id).map_err(CliError::from)?;
    let sigint = SIGINT_COUNT.load(Ordering::SeqCst);
    emit_job_result(json, &job, sigint, completed_count)?;
    job_to_result(job, sigint)
}

fn wait_for_terminal(
    runner: &ProcessRunner,
    progress: &mut tokio::sync::watch::Receiver<JobProgressSnapshot>,
    job_id: &str,
) -> Result<()> {
    loop {
        if SIGINT_FORCE.load(Ordering::SeqCst) {
            // Last resort: cancel again then leave wait (shutdown will join).
            let _ = runner.cancel(job_id);
            eprintln!("signal: force — exiting wait after cancel request");
            break;
        }
        if SIGINT_COUNT.load(Ordering::SeqCst) {
            let _ = runner.cancel(job_id);
        }

        if !runner.is_busy() {
            // Drain final snapshot.
            let snap = progress.borrow().clone();
            if !snap.job_id.is_empty() {
                eprintln!(
                    "progress: state={} stage={:?} count={} msg={:?}",
                    snap.state, snap.stage, snap.completed_count, snap.message
                );
            }
            break;
        }

        if progress.has_changed().unwrap_or(false) {
            let snap = progress.borrow_and_update().clone();
            eprintln!(
                "progress: state={} stage={:?} count={} msg={:?}",
                snap.state, snap.stage, snap.completed_count, snap.message
            );
        } else {
            // Periodic line even without change (operator feedback).
            let snap = progress.borrow().clone();
            if snap.job_id == job_id {
                eprintln!(
                    "progress: state={} stage={:?} count={}",
                    snap.state, snap.stage, snap.completed_count
                );
            }
        }
        thread::sleep(Duration::from_millis(200));
    }

    // Brief settle for final state write.
    let _ = runner.wait_until_idle(Duration::from_secs(30));
    Ok(())
}

fn emit_job_result(
    json: bool,
    job: &Job,
    sigint_requested: bool,
    completed_count: u64,
) -> Result<()> {
    let interrupted_pause = sigint_requested && job.state == JobState::Paused;
    if json {
        if matches!(job.state, JobState::Succeeded)
            || (job.state == JobState::Paused && !sigint_requested)
        {
            let envelope = ok_envelope(json!({
                "job_id": job.id,
                "kind": job.kind,
                "state": job.state.as_str(),
                "message": job.error_summary,
                "completed_count": completed_count,
                "parent_job_id": job.parent_job_id,
            }));
            emit_json(true, &envelope)?;
        } else if matches!(job.state, JobState::Failed | JobState::Cancelled) || interrupted_pause {
            let code = if interrupted_pause || job.state == JobState::Cancelled {
                "cancelled"
            } else {
                "job_failed"
            };
            let env = json!({
                "ok": false,
                "error": {
                    "code": code,
                    "message": job.error_summary.clone().unwrap_or_else(|| {
                        if interrupted_pause {
                            "job paused after cancel (SIGINT)".into()
                        } else {
                            job.state.as_str().into()
                        }
                    }),
                },
                "job_id": job.id,
                "state": job.state.as_str(),
                "kind": job.kind,
                "completed_count": completed_count,
                "parent_job_id": job.parent_job_id,
            });
            emit_json(true, &env)?;
        } else {
            let envelope = ok_envelope(json!({
                "job_id": job.id,
                "kind": job.kind,
                "state": job.state.as_str(),
                "message": job.error_summary,
                "completed_count": completed_count,
                "parent_job_id": job.parent_job_id,
            }));
            emit_json(true, &envelope)?;
        }
    } else {
        println!(
            "job {} kind={} state={} completed={} parent={:?}",
            job.id, job.kind, job.state, completed_count, job.parent_job_id
        );
        if let Some(ref m) = job.error_summary {
            println!("  message: {m}");
        }
    }
    Ok(())
}

fn job_to_result(job: Job, sigint_requested: bool) -> Result<Job> {
    // Ctrl+C cooperative cancel often ends as Paused — scripts treat that as non-success.
    if sigint_requested && matches!(job.state, JobState::Paused | JobState::Cancelled) {
        return Err(CliError::JobFailed {
            message: job
                .error_summary
                .clone()
                .unwrap_or_else(|| "job interrupted (SIGINT)".into()),
            job_id: Some(job.id),
            state: Some(job.state.as_str().to_string()),
        });
    }
    match job.state {
        JobState::Succeeded | JobState::Paused => Ok(job),
        JobState::Failed | JobState::Cancelled => Err(CliError::JobFailed {
            message: job
                .error_summary
                .clone()
                .unwrap_or_else(|| format!("job {}", job.state)),
            job_id: Some(job.id),
            state: Some(job.state.as_str().to_string()),
        }),
        other => Err(CliError::Msg(format!(
            "job {} ended in unexpected state {other}",
            job.id
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sigint_install_fails_closed_on_second_call() {
        let slot = Arc::new(std::sync::Mutex::new(None));
        // First install may succeed or fail if another test already installed.
        let first = install_sigint_handler(Arc::clone(&slot));
        let second = install_sigint_handler(slot);
        // At least one of the subsequent installs must fail closed (Msg).
        if first.is_ok() {
            assert!(
                second.is_err(),
                "second SIGINT install must fail closed after success"
            );
        }
    }
}
