//! Matter-level offline semantic index handler (`matter-semantic`).

use matter_semantic::{
    run_semantic_index, SemanticIndexParams, SemanticOutcome, JOB_KIND_SEMANTIC_INDEX,
};

use crate::error::RunnerError;
use crate::handler::{JobContext, JobHandler, JobOutcome};

/// Handler for matter-level semantic index (`kind = "semantic_index"`).
pub struct MatterSemanticIndexHandler;

impl Default for MatterSemanticIndexHandler {
    fn default() -> Self {
        Self
    }
}

impl MatterSemanticIndexHandler {
    pub fn new() -> Self {
        Self
    }
}

impl JobHandler for MatterSemanticIndexHandler {
    fn kind(&self) -> &'static str {
        JOB_KIND_SEMANTIC_INDEX
    }

    fn run(&self, ctx: &JobContext<'_>) -> Result<JobOutcome, RunnerError> {
        let params = SemanticIndexParams::from_json(ctx.params_json)
            .map_err(|e| RunnerError::InvalidParams(e.to_string()))?;

        let cancel_fn = ctx.cancel.as_fn();
        let cancel: Option<&dyn Fn() -> bool> = Some(&cancel_fn);

        ctx.progress.patch(|s| {
            s.stage = Some("semantic_index".into());
            s.message = Some(if ctx.is_resume {
                "resume semantic_index".into()
            } else {
                "semantic_index".into()
            });
        });

        let progress_sink = ctx.progress.clone();
        let outcome = run_semantic_index(ctx.matter, ctx.job_id, &params, cancel, |completed| {
            progress_sink.patch(|s| {
                s.completed_count = completed;
                s.stage = Some("semantic_index".into());
            });
        })
        .map_err(|e| RunnerError::HandlerFailed(e.to_string()))?;

        Ok(map_outcome(outcome))
    }
}

fn map_outcome(outcome: SemanticOutcome) -> JobOutcome {
    match outcome {
        SemanticOutcome::Succeeded(r) => JobOutcome::Succeeded {
            message: Some(format!(
                "semantic_index embedded={} skipped={} cleared={} errors={} chunks={} model={}",
                r.embedded_count,
                r.skipped_count,
                r.cleared_count,
                r.error_count,
                r.total_chunks,
                r.model_id
            )),
            completed_count: r.completed_count,
        },
        SemanticOutcome::Paused(s) => JobOutcome::Paused {
            message: Some("cancelled".into()),
            completed_count: s.completed_count,
        },
        SemanticOutcome::Failed { message, summary } => JobOutcome::Failed {
            message: format!(
                "{message} (embedded={} skipped={} errors={})",
                summary.embedded_count, summary.skipped_count, summary.error_count
            ),
        },
    }
}
