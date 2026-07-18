//! Matter-level near-duplicate detection handler (`matter-neardup`).

use matter_neardup::{run_neardup, NearDupOutcome, NearDupParams, JOB_KIND_NEARDUP};

use crate::error::RunnerError;
use crate::handler::{JobContext, JobHandler, JobOutcome};

/// Handler for matter-level near-dup (`kind = "neardup"`).
pub struct MatterNearDupHandler;

impl Default for MatterNearDupHandler {
    fn default() -> Self {
        Self
    }
}

impl MatterNearDupHandler {
    pub fn new() -> Self {
        Self
    }
}

impl JobHandler for MatterNearDupHandler {
    fn kind(&self) -> &'static str {
        JOB_KIND_NEARDUP
    }

    fn run(&self, ctx: &JobContext<'_>) -> Result<JobOutcome, RunnerError> {
        let params = NearDupParams::from_json(ctx.params_json)
            .map_err(|e| RunnerError::InvalidParams(e.to_string()))?;

        let cancel_fn = ctx.cancel.as_fn();
        let cancel: Option<&dyn Fn() -> bool> = Some(&cancel_fn);

        ctx.progress.patch(|s| {
            s.stage = Some("neardup".into());
            s.message = Some(if ctx.is_resume {
                "resume neardup".into()
            } else {
                "neardup".into()
            });
        });

        let progress_sink = ctx.progress.clone();
        let outcome = run_neardup(ctx.matter, ctx.job_id, &params, cancel, |completed| {
            progress_sink.patch(|s| {
                s.completed_count = completed;
                s.stage = Some("neardup".into());
            });
        })
        .map_err(|e| RunnerError::HandlerFailed(e.to_string()))?;

        Ok(map_outcome(outcome))
    }
}

fn map_outcome(outcome: NearDupOutcome) -> JobOutcome {
    match outcome {
        NearDupOutcome::Succeeded(s) => JobOutcome::Succeeded {
            message: Some(format!(
                "groups={} members={} unique={} skipped={}",
                s.group_count, s.member_count, s.unique_count, s.skipped_count
            )),
            completed_count: s.completed_count,
        },
        NearDupOutcome::Paused(s) => JobOutcome::Paused {
            message: Some("cancelled".into()),
            completed_count: s.completed_count,
        },
        NearDupOutcome::Failed { message, summary } => JobOutcome::Failed {
            message: format!(
                "{message} (completed={} groups={})",
                summary.completed_count, summary.group_count
            ),
        },
    }
}
