//! Matter-level email threading handler (`matter-thread`).

use matter_thread::{run_thread, ThreadOutcome, ThreadParams, JOB_KIND_THREAD};

use crate::error::RunnerError;
use crate::handler::{JobContext, JobHandler, JobOutcome};

/// Handler for matter-level threading (`kind = "thread"`).
pub struct MatterThreadHandler;

impl Default for MatterThreadHandler {
    fn default() -> Self {
        Self
    }
}

impl MatterThreadHandler {
    pub fn new() -> Self {
        Self
    }
}

impl JobHandler for MatterThreadHandler {
    fn kind(&self) -> &'static str {
        JOB_KIND_THREAD
    }

    fn run(&self, ctx: &JobContext<'_>) -> Result<JobOutcome, RunnerError> {
        let params = ThreadParams::from_json(ctx.params_json)
            .map_err(|e| RunnerError::InvalidParams(e.to_string()))?;

        let cancel_fn = ctx.cancel.as_fn();
        let cancel: Option<&dyn Fn() -> bool> = Some(&cancel_fn);

        ctx.progress.patch(|s| {
            s.stage = Some("thread".into());
            s.message = Some(if ctx.is_resume {
                "resume thread".into()
            } else {
                "thread".into()
            });
        });

        let progress_sink = ctx.progress.clone();
        let outcome = run_thread(ctx.matter, ctx.job_id, &params, cancel, |completed| {
            progress_sink.patch(|s| {
                s.completed_count = completed;
                s.stage = Some("thread".into());
            });
        })
        .map_err(|e| RunnerError::HandlerFailed(e.to_string()))?;

        Ok(map_outcome(outcome))
    }
}

fn map_outcome(outcome: ThreadOutcome) -> JobOutcome {
    match outcome {
        ThreadOutcome::Succeeded(s) => JobOutcome::Succeeded {
            message: Some(format!(
                "threads={} header={} subject={} index={} singleton={}",
                s.thread_count, s.header_linked, s.subject_linked, s.index_linked, s.singleton
            )),
            completed_count: s.completed_count,
        },
        ThreadOutcome::Paused(s) => JobOutcome::Paused {
            message: Some("cancelled".into()),
            completed_count: s.completed_count,
        },
        ThreadOutcome::Failed { message, summary } => JobOutcome::Failed {
            message: format!(
                "{message} (completed={} threads={})",
                summary.completed_count, summary.thread_count
            ),
        },
    }
}
