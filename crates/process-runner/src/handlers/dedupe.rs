//! Matter-level tiered dedupe handler (`matter-dedupe`).

use matter_dedupe::{run_dedupe, DedupeOutcome, DedupeParams, JOB_KIND_DEDUPE};

use crate::error::RunnerError;
use crate::handler::{JobContext, JobHandler, JobOutcome};

/// Handler for matter-level dedupe (`kind = "dedupe"`).
pub struct MatterDedupeHandler;

impl Default for MatterDedupeHandler {
    fn default() -> Self {
        Self
    }
}

impl MatterDedupeHandler {
    pub fn new() -> Self {
        Self
    }
}

impl JobHandler for MatterDedupeHandler {
    fn kind(&self) -> &'static str {
        JOB_KIND_DEDUPE
    }

    fn run(&self, ctx: &JobContext<'_>) -> Result<JobOutcome, RunnerError> {
        let params = DedupeParams::from_json(ctx.params_json)
            .map_err(|e| RunnerError::InvalidParams(e.to_string()))?;

        let cancel_fn = ctx.cancel.as_fn();
        let cancel: Option<&dyn Fn() -> bool> = Some(&cancel_fn);

        ctx.progress.patch(|s| {
            s.stage = Some("dedupe".into());
            s.message = Some(if ctx.is_resume {
                "resume dedupe".into()
            } else {
                "dedupe".into()
            });
        });

        let progress_sink = ctx.progress.clone();
        let outcome = run_dedupe(ctx.matter, ctx.job_id, &params, cancel, |completed| {
            progress_sink.patch(|s| {
                s.completed_count = completed;
                s.stage = Some("dedupe".into());
            });
        })
        .map_err(|e| RunnerError::HandlerFailed(e.to_string()))?;

        Ok(map_outcome(outcome))
    }
}

fn map_outcome(outcome: DedupeOutcome) -> JobOutcome {
    match outcome {
        DedupeOutcome::Succeeded(s) => JobOutcome::Succeeded {
            message: Some(format!(
                "unique={} duplicate={} skipped={} mid_logical_conflicts={}",
                s.unique, s.duplicate, s.skipped, s.mid_logical_conflicts
            )),
            completed_count: s.completed_count,
        },
        DedupeOutcome::Paused(s) => JobOutcome::Paused {
            message: Some("cancelled".into()),
            completed_count: s.completed_count,
        },
        DedupeOutcome::Failed { message, summary } => JobOutcome::Failed {
            message: format!(
                "{message} (unique={} duplicate={})",
                summary.unique, summary.duplicate
            ),
        },
    }
}
