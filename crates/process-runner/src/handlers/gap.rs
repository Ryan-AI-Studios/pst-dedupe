//! Matter-level gap analysis handler (`matter-gap`).

use matter_gap::{run_gap, GapOutcome, GapParams, JOB_KIND_GAP};

use crate::error::RunnerError;
use crate::handler::{JobContext, JobHandler, JobOutcome};

/// Handler for matter-level gap analysis (`kind = "gap"`).
pub struct MatterGapHandler;

impl Default for MatterGapHandler {
    fn default() -> Self {
        Self
    }
}

impl MatterGapHandler {
    pub fn new() -> Self {
        Self
    }
}

impl JobHandler for MatterGapHandler {
    fn kind(&self) -> &'static str {
        JOB_KIND_GAP
    }

    fn run(&self, ctx: &JobContext<'_>) -> Result<JobOutcome, RunnerError> {
        let params = GapParams::from_json(ctx.params_json)
            .map_err(|e| RunnerError::InvalidParams(e.to_string()))?;

        let cancel_fn = ctx.cancel.as_fn();
        let cancel: Option<&dyn Fn() -> bool> = Some(&cancel_fn);

        ctx.progress.patch(|s| {
            s.stage = Some("gap".into());
            s.message = Some(if ctx.is_resume {
                "resume gap".into()
            } else {
                "gap analysis".into()
            });
        });

        let progress_sink = ctx.progress.clone();
        let outcome = run_gap(ctx.matter, ctx.job_id, &params, cancel, |completed| {
            progress_sink.patch(|s| {
                s.completed_count = completed;
                s.stage = Some("gap".into());
            });
        })
        .map_err(|e| RunnerError::HandlerFailed(e.to_string()))?;

        Ok(map_outcome(outcome))
    }
}

fn map_outcome(outcome: GapOutcome) -> JobOutcome {
    match outcome {
        GapOutcome::Succeeded(r) => JobOutcome::Succeeded {
            message: Some(format!(
                "gap kind={} errors={} warns={} findings={} → {}",
                r.kind, r.error_count, r.warn_count, r.finding_count, r.report_path
            )),
            completed_count: r.expected_doc_count.max(r.finding_count),
        },
        GapOutcome::Paused(s) => JobOutcome::Paused {
            message: Some("cancelled".into()),
            completed_count: s.completed_count,
        },
        GapOutcome::Failed { message, summary } => JobOutcome::Failed {
            message: format!(
                "{message} (errors={} warns={})",
                summary.error_count, summary.warn_count
            ),
        },
    }
}
