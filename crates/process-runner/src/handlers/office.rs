//! Matter-level office extract handler (`extract-office`).

use extract_office::{
    run_office_extract, OfficeExtractOutcome, OfficeExtractParams, JOB_KIND_OFFICE_EXTRACT,
};

use crate::error::RunnerError;
use crate::handler::{JobContext, JobHandler, JobOutcome};

/// Handler for matter-level office extract (`kind = "office_extract"`).
pub struct MatterOfficeExtractHandler;

impl Default for MatterOfficeExtractHandler {
    fn default() -> Self {
        Self
    }
}

impl MatterOfficeExtractHandler {
    pub fn new() -> Self {
        Self
    }
}

impl JobHandler for MatterOfficeExtractHandler {
    fn kind(&self) -> &'static str {
        JOB_KIND_OFFICE_EXTRACT
    }

    fn run(&self, ctx: &JobContext<'_>) -> Result<JobOutcome, RunnerError> {
        let params = OfficeExtractParams::from_json(ctx.params_json)
            .map_err(|e| RunnerError::InvalidParams(e.to_string()))?;

        let cancel_fn = ctx.cancel.as_fn();
        let cancel: Option<&dyn Fn() -> bool> = Some(&cancel_fn);

        ctx.progress.patch(|s| {
            s.stage = Some("office_extract".into());
            s.message = Some(if ctx.is_resume {
                "resume office_extract".into()
            } else {
                "office_extract".into()
            });
        });

        let progress_sink = ctx.progress.clone();
        let outcome = run_office_extract(ctx.matter, ctx.job_id, &params, cancel, |completed| {
            progress_sink.patch(|s| {
                s.completed_count = completed;
                s.stage = Some("office_extract".into());
            });
        })
        .map_err(|e| RunnerError::HandlerFailed(e.to_string()))?;

        Ok(map_outcome(outcome))
    }
}

fn map_outcome(outcome: OfficeExtractOutcome) -> JobOutcome {
    match outcome {
        OfficeExtractOutcome::Succeeded(s) => JobOutcome::Succeeded {
            message: Some(format!(
                "extracted={} skipped={} errors={}",
                s.extracted_count, s.skipped_count, s.error_count
            )),
            completed_count: s.completed_count,
        },
        OfficeExtractOutcome::Paused(s) => JobOutcome::Paused {
            message: Some("cancelled".into()),
            completed_count: s.completed_count,
        },
        OfficeExtractOutcome::Failed { message, summary } => JobOutcome::Failed {
            message: format!(
                "{message} (completed={} extracted={})",
                summary.completed_count, summary.extracted_count
            ),
        },
    }
}
