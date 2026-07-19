//! Matter-level production export handler (`matter-produce`).

use matter_produce::{
    run_produce, ProduceOutcome, ProduceParams, JOB_KIND_PRODUCE, JOB_KIND_PRODUCTION_EXPORT,
};

use crate::error::RunnerError;
use crate::handler::{JobContext, JobHandler, JobOutcome};

/// Handler for matter-level produce (`kind = "produce"`).
pub struct MatterProduceHandler;

impl Default for MatterProduceHandler {
    fn default() -> Self {
        Self
    }
}

impl MatterProduceHandler {
    pub fn new() -> Self {
        Self
    }
}

impl JobHandler for MatterProduceHandler {
    fn kind(&self) -> &'static str {
        JOB_KIND_PRODUCE
    }

    fn run(&self, ctx: &JobContext<'_>) -> Result<JobOutcome, RunnerError> {
        let params = ProduceParams::from_json(ctx.params_json)
            .map_err(|e| RunnerError::InvalidParams(e.to_string()))?;

        let cancel_fn = ctx.cancel.as_fn();
        let cancel: Option<&dyn Fn() -> bool> = Some(&cancel_fn);

        ctx.progress.patch(|s| {
            s.stage = Some("produce".into());
            s.message = Some(if ctx.is_resume {
                "resume produce".into()
            } else {
                "produce".into()
            });
        });

        let progress_sink = ctx.progress.clone();
        let outcome = run_produce(ctx.matter, ctx.job_id, &params, cancel, |completed| {
            progress_sink.patch(|s| {
                s.completed_count = completed;
                s.stage = Some("produce".into());
            });
        })
        .map_err(|e| RunnerError::HandlerFailed(e.to_string()))?;

        Ok(map_outcome(outcome))
    }
}

/// Alias handler so `kind = "production_export"` also works.
pub struct MatterProductionExportHandler;

impl Default for MatterProductionExportHandler {
    fn default() -> Self {
        Self
    }
}

impl MatterProductionExportHandler {
    pub fn new() -> Self {
        Self
    }
}

impl JobHandler for MatterProductionExportHandler {
    fn kind(&self) -> &'static str {
        JOB_KIND_PRODUCTION_EXPORT
    }

    fn run(&self, ctx: &JobContext<'_>) -> Result<JobOutcome, RunnerError> {
        MatterProduceHandler.run(ctx)
    }
}

fn map_outcome(outcome: ProduceOutcome) -> JobOutcome {
    match outcome {
        ProduceOutcome::Succeeded(s) => JobOutcome::Succeeded {
            message: Some(format!(
                "produced={} withheld_skip={} errors={} → {}",
                s.produced_count, s.skipped_withheld, s.error_count, s.output_root
            )),
            completed_count: s.completed_count,
        },
        ProduceOutcome::Paused(s) => JobOutcome::Paused {
            message: Some("cancelled".into()),
            completed_count: s.completed_count,
        },
        ProduceOutcome::Failed { message, summary } => JobOutcome::Failed {
            message: format!(
                "{message} (produced={} withheld_skip={} errors={})",
                summary.produced_count, summary.skipped_withheld, summary.error_count
            ),
        },
    }
}
