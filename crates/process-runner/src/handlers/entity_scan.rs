//! Matter-level entity / PII pack scan handler (`matter-entity`).

use matter_entity::{run_entity_scan, EntityScanOutcome, EntityScanParams, JOB_KIND_ENTITY_SCAN};

use crate::error::RunnerError;
use crate::handler::{JobContext, JobHandler, JobOutcome};

/// Handler for matter-level entity scan (`kind = "entity_scan"`).
pub struct MatterEntityScanHandler;

impl Default for MatterEntityScanHandler {
    fn default() -> Self {
        Self
    }
}

impl MatterEntityScanHandler {
    pub fn new() -> Self {
        Self
    }
}

impl JobHandler for MatterEntityScanHandler {
    fn kind(&self) -> &'static str {
        JOB_KIND_ENTITY_SCAN
    }

    fn run(&self, ctx: &JobContext<'_>) -> Result<JobOutcome, RunnerError> {
        let params = EntityScanParams::from_json(ctx.params_json)
            .map_err(|e| RunnerError::InvalidParams(e.to_string()))?;

        let cancel_fn = ctx.cancel.as_fn();
        let cancel: Option<&dyn Fn() -> bool> = Some(&cancel_fn);

        ctx.progress.patch(|s| {
            s.stage = Some("entity_scan".into());
            s.message = Some(if ctx.is_resume {
                "resume entity_scan".into()
            } else {
                "entity_scan".into()
            });
        });

        let progress_sink = ctx.progress.clone();
        let outcome = run_entity_scan(ctx.matter, ctx.job_id, &params, cancel, |completed| {
            progress_sink.patch(|s| {
                s.completed_count = completed;
                s.stage = Some("entity_scan".into());
            });
        })
        .map_err(|e| RunnerError::HandlerFailed(e.to_string()))?;

        Ok(map_outcome(outcome))
    }
}

fn map_outcome(outcome: EntityScanOutcome) -> JobOutcome {
    match outcome {
        EntityScanOutcome::Succeeded(r) => JobOutcome::Succeeded {
            message: Some(format!(
                "entity_scan scanned={} skipped={} hits={} errors={} trunc={}",
                r.scanned_count, r.skipped_count, r.hit_count, r.error_count, r.truncated_count
            )),
            completed_count: r.completed_count,
        },
        EntityScanOutcome::Paused(s) => JobOutcome::Paused {
            message: Some("cancelled".into()),
            completed_count: s.completed_count,
        },
        EntityScanOutcome::Failed { message, summary } => JobOutcome::Failed {
            message: format!(
                "{message} (scanned={} hits={} errors={})",
                summary.scanned_count, summary.hit_count, summary.error_count
            ),
        },
    }
}
