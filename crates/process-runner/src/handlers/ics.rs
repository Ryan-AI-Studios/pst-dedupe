//! Matter-level ICS extract handler (`extract-calendar`).

use extract_calendar::{
    run_ics_extract, IcsExtractOutcome, IcsExtractParams, JOB_KIND_ICS_EXTRACT,
};

use crate::error::RunnerError;
use crate::handler::{JobContext, JobHandler, JobOutcome};

/// Handler for matter-level ICS extract (`kind = "ics_extract"`).
pub struct MatterIcsExtractHandler;

impl Default for MatterIcsExtractHandler {
    fn default() -> Self {
        Self
    }
}

impl MatterIcsExtractHandler {
    pub fn new() -> Self {
        Self
    }
}

impl JobHandler for MatterIcsExtractHandler {
    fn kind(&self) -> &'static str {
        JOB_KIND_ICS_EXTRACT
    }

    fn run(&self, ctx: &JobContext<'_>) -> Result<JobOutcome, RunnerError> {
        let params = IcsExtractParams::from_json(ctx.params_json)
            .map_err(|e| RunnerError::InvalidParams(e.to_string()))?;

        let cancel_fn = ctx.cancel.as_fn();
        let cancel: Option<&dyn Fn() -> bool> = Some(&cancel_fn);

        ctx.progress.patch(|s| {
            s.stage = Some("ics_extract".into());
            s.message = Some(if ctx.is_resume {
                "resume ics_extract".into()
            } else {
                "ics_extract".into()
            });
        });

        let progress_sink = ctx.progress.clone();
        let outcome = run_ics_extract(ctx.matter, ctx.job_id, &params, cancel, |completed| {
            progress_sink.patch(|s| {
                s.completed_count = completed;
                s.stage = Some("ics_extract".into());
            });
        })
        .map_err(|e| RunnerError::HandlerFailed(e.to_string()))?;

        Ok(map_outcome(outcome))
    }
}

fn map_outcome(outcome: IcsExtractOutcome) -> JobOutcome {
    match outcome {
        IcsExtractOutcome::Succeeded(s) => JobOutcome::Succeeded {
            message: Some(format!(
                "extracted={} skipped={} errors={} children={}",
                s.extracted_count, s.skipped_count, s.error_count, s.child_count
            )),
            completed_count: s.completed_count,
        },
        IcsExtractOutcome::Paused(s) => JobOutcome::Paused {
            message: Some("cancelled".into()),
            completed_count: s.completed_count,
        },
        IcsExtractOutcome::Failed { message, summary } => JobOutcome::Failed {
            message: format!(
                "{message} (completed={} extracted={})",
                summary.completed_count, summary.extracted_count
            ),
        },
    }
}
