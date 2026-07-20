//! Matter-level cull / data-reduction handler (`matter-cull`).

use matter_cull::{run_cull, CullOutcome, CullParams, JOB_KIND_CULL};

use crate::error::RunnerError;
use crate::handler::{JobContext, JobHandler, JobOutcome};

/// Handler for matter-level cull (`kind = "cull"`).
pub struct MatterCullHandler;

impl Default for MatterCullHandler {
    fn default() -> Self {
        Self
    }
}

impl MatterCullHandler {
    pub fn new() -> Self {
        Self
    }
}

impl JobHandler for MatterCullHandler {
    fn kind(&self) -> &'static str {
        JOB_KIND_CULL
    }

    fn run(&self, ctx: &JobContext<'_>) -> Result<JobOutcome, RunnerError> {
        let params = CullParams::from_json(ctx.params_json)
            .map_err(|e| RunnerError::InvalidParams(e.to_string()))?;

        let cancel_fn = ctx.cancel.as_fn();
        let cancel: Option<&dyn Fn() -> bool> = Some(&cancel_fn);

        ctx.progress.patch(|s| {
            s.stage = Some("cull".into());
            s.message = Some(if ctx.is_resume {
                "resume cull".into()
            } else {
                "cull".into()
            });
        });

        let progress_sink = ctx.progress.clone();
        let outcome = run_cull(ctx.matter, ctx.job_id, &params, cancel, |completed| {
            progress_sink.patch(|s| {
                s.completed_count = completed;
                s.stage = Some("cull".into());
            });
        })
        .map_err(|e| RunnerError::HandlerFailed(e.to_string()))?;

        Ok(map_outcome(outcome))
    }
}

fn map_outcome(outcome: CullOutcome) -> JobOutcome {
    match outcome {
        CullOutcome::Succeeded(s) => JobOutcome::Succeeded {
            message: Some(format!(
                "included={} culled={} skipped={} completed={}",
                s.included, s.culled, s.skipped, s.completed_count
            )),
            completed_count: s.completed_count,
        },
        CullOutcome::Paused(s) => JobOutcome::Paused {
            message: Some("cancelled".into()),
            completed_count: s.completed_count,
        },
        CullOutcome::Failed { message, summary } => JobOutcome::Failed {
            message: format!(
                "{message} (completed={} included={} culled={})",
                summary.completed_count, summary.included, summary.culled
            ),
        },
    }
}
