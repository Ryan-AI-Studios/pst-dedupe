//! Matter-level FTS index handler (`matter-search`).

use matter_search::{run_fts_index, FtsIndexParams, FtsOutcome, JOB_KIND_FTS_INDEX};

use crate::error::RunnerError;
use crate::handler::{JobContext, JobHandler, JobOutcome};

/// Handler for matter-level FTS index (`kind = "fts_index"`).
pub struct MatterFtsIndexHandler;

impl Default for MatterFtsIndexHandler {
    fn default() -> Self {
        Self
    }
}

impl MatterFtsIndexHandler {
    pub fn new() -> Self {
        Self
    }
}

impl JobHandler for MatterFtsIndexHandler {
    fn kind(&self) -> &'static str {
        JOB_KIND_FTS_INDEX
    }

    fn run(&self, ctx: &JobContext<'_>) -> Result<JobOutcome, RunnerError> {
        let params = FtsIndexParams::from_json(ctx.params_json)
            .map_err(|e| RunnerError::InvalidParams(e.to_string()))?;

        let cancel_fn = ctx.cancel.as_fn();
        let cancel: Option<&dyn Fn() -> bool> = Some(&cancel_fn);

        ctx.progress.patch(|s| {
            s.stage = Some("fts_index".into());
            s.message = Some(if ctx.is_resume {
                "resume fts_index".into()
            } else {
                "fts_index".into()
            });
        });

        let progress_sink = ctx.progress.clone();
        let outcome = run_fts_index(ctx.matter, ctx.job_id, &params, cancel, |completed| {
            progress_sink.patch(|s| {
                s.completed_count = completed;
                s.stage = Some("fts_index".into());
            });
        })
        .map_err(|e| RunnerError::HandlerFailed(e.to_string()))?;

        Ok(map_outcome(outcome))
    }
}

fn map_outcome(outcome: FtsOutcome) -> JobOutcome {
    match outcome {
        FtsOutcome::Succeeded(s) => JobOutcome::Succeeded {
            message: Some(format!(
                "indexed={} skipped={} errors={}",
                s.indexed_count, s.skipped_count, s.error_count
            )),
            completed_count: s.completed_count,
        },
        FtsOutcome::Paused(s) => JobOutcome::Paused {
            message: Some("cancelled".into()),
            completed_count: s.completed_count,
        },
        FtsOutcome::Failed { message, summary } => JobOutcome::Failed {
            message: format!(
                "{message} (completed={} indexed={})",
                summary.completed_count, summary.indexed_count
            ),
        },
    }
}
