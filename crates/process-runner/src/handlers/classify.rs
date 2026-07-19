//! Matter-level file-category classify handler (`file-category`).

use file_category::{run_classify, ClassifyOutcome, ClassifyParams, JOB_KIND_CLASSIFY};

use crate::error::RunnerError;
use crate::handler::{JobContext, JobHandler, JobOutcome};

/// Handler for matter-level classify (`kind = "classify"`).
pub struct MatterClassifyHandler;

impl Default for MatterClassifyHandler {
    fn default() -> Self {
        Self
    }
}

impl MatterClassifyHandler {
    pub fn new() -> Self {
        Self
    }
}

impl JobHandler for MatterClassifyHandler {
    fn kind(&self) -> &'static str {
        JOB_KIND_CLASSIFY
    }

    fn run(&self, ctx: &JobContext<'_>) -> Result<JobOutcome, RunnerError> {
        let params = ClassifyParams::from_json(ctx.params_json)
            .map_err(|e| RunnerError::InvalidParams(e.to_string()))?;

        let cancel_fn = ctx.cancel.as_fn();
        let cancel: Option<&dyn Fn() -> bool> = Some(&cancel_fn);

        ctx.progress.patch(|s| {
            s.stage = Some("classify".into());
            s.message = Some(if ctx.is_resume {
                "resume classify".into()
            } else {
                "classify".into()
            });
        });

        let progress_sink = ctx.progress.clone();
        let outcome = run_classify(ctx.matter, ctx.job_id, &params, cancel, |completed| {
            progress_sink.patch(|s| {
                s.completed_count = completed;
                s.stage = Some("classify".into());
            });
        })
        .map_err(|e| RunnerError::HandlerFailed(e.to_string()))?;

        Ok(map_outcome(outcome))
    }
}

fn map_outcome(outcome: ClassifyOutcome) -> JobOutcome {
    match outcome {
        ClassifyOutcome::Succeeded(s) => JobOutcome::Succeeded {
            message: Some(format!(
                "classified={} skipped={} errors={}",
                s.classified_count, s.skipped_count, s.error_count
            )),
            completed_count: s.completed_count,
        },
        ClassifyOutcome::Paused(s) => JobOutcome::Paused {
            message: Some("cancelled".into()),
            completed_count: s.completed_count,
        },
        ClassifyOutcome::Failed { message, summary } => JobOutcome::Failed {
            message: format!(
                "{message} (completed={} classified={})",
                summary.completed_count, summary.classified_count
            ),
        },
    }
}
