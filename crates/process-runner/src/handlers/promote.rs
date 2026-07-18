//! Matter-level promote-to-review handler (`matter-promote`).

use matter_promote::{run_promote, PromoteOutcome, PromoteParams, JOB_KIND_PROMOTE};

use crate::error::RunnerError;
use crate::handler::{JobContext, JobHandler, JobOutcome};

/// Handler for matter-level promote (`kind = "promote"`).
pub struct MatterPromoteHandler;

impl Default for MatterPromoteHandler {
    fn default() -> Self {
        Self
    }
}

impl MatterPromoteHandler {
    pub fn new() -> Self {
        Self
    }
}

impl JobHandler for MatterPromoteHandler {
    fn kind(&self) -> &'static str {
        JOB_KIND_PROMOTE
    }

    fn run(&self, ctx: &JobContext<'_>) -> Result<JobOutcome, RunnerError> {
        let params = PromoteParams::from_json(ctx.params_json)
            .map_err(|e| RunnerError::InvalidParams(e.to_string()))?;

        let cancel_fn = ctx.cancel.as_fn();
        let cancel: Option<&dyn Fn() -> bool> = Some(&cancel_fn);

        ctx.progress.patch(|s| {
            s.stage = Some("promote".into());
            s.message = Some(if ctx.is_resume {
                "resume promote".into()
            } else {
                "promote".into()
            });
        });

        let progress_sink = ctx.progress.clone();
        let outcome = run_promote(ctx.matter, ctx.job_id, &params, cancel, |completed| {
            progress_sink.patch(|s| {
                s.completed_count = completed;
                s.stage = Some("promote".into());
            });
        })
        .map_err(|e| RunnerError::HandlerFailed(e.to_string()))?;

        Ok(map_outcome(outcome))
    }
}

fn map_outcome(outcome: PromoteOutcome) -> JobOutcome {
    match outcome {
        PromoteOutcome::Succeeded(s) => JobOutcome::Succeeded {
            message: Some(format!(
                "promoted={} policy={} set={}",
                s.promoted_count, s.resolved_policy, s.review_set_name
            )),
            completed_count: s.completed_count,
        },
        PromoteOutcome::Paused(s) => JobOutcome::Paused {
            message: Some("cancelled".into()),
            completed_count: s.completed_count,
        },
        PromoteOutcome::Failed { message, summary } => JobOutcome::Failed {
            message: format!(
                "{message} (completed={} promoted={} policy={})",
                summary.completed_count, summary.promoted_count, summary.resolved_policy
            ),
        },
    }
}
