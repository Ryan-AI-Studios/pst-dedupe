//! Matter-level production QC handler (`matter-qc`).

use matter_qc::{run_production_qc, QcOutcome, QcParams, JOB_KIND_QC};

use crate::error::RunnerError;
use crate::handler::{JobContext, JobHandler, JobOutcome};

/// Handler for matter-level production QC (`kind = "qc"`).
pub struct MatterQcHandler;

impl Default for MatterQcHandler {
    fn default() -> Self {
        Self
    }
}

impl MatterQcHandler {
    pub fn new() -> Self {
        Self
    }
}

impl JobHandler for MatterQcHandler {
    fn kind(&self) -> &'static str {
        JOB_KIND_QC
    }

    fn run(&self, ctx: &JobContext<'_>) -> Result<JobOutcome, RunnerError> {
        let params = QcParams::from_json(ctx.params_json)
            .map_err(|e| RunnerError::InvalidParams(e.to_string()))?;

        let cancel_fn = ctx.cancel.as_fn();
        let cancel: Option<&dyn Fn() -> bool> = Some(&cancel_fn);

        ctx.progress.patch(|s| {
            s.stage = Some("qc".into());
            s.message = Some(if ctx.is_resume {
                "resume qc".into()
            } else {
                "production qc".into()
            });
        });

        let progress_sink = ctx.progress.clone();
        let outcome = run_production_qc(ctx.matter, ctx.job_id, &params, cancel, |completed| {
            progress_sink.patch(|s| {
                s.completed_count = completed;
                s.stage = Some("qc".into());
            });
        })
        .map_err(|e| RunnerError::HandlerFailed(e.to_string()))?;

        Ok(map_outcome(outcome))
    }
}

fn map_outcome(outcome: QcOutcome) -> JobOutcome {
    match outcome {
        QcOutcome::Succeeded(r) => JobOutcome::Succeeded {
            message: Some(format!(
                "qc passed={} errors={} warns={} candidates={} → {}",
                r.passed, r.error_count, r.warn_count, r.candidate_count, r.report_path
            )),
            completed_count: r.candidate_count,
        },
        QcOutcome::Paused(s) => JobOutcome::Paused {
            message: Some("cancelled".into()),
            completed_count: s.completed_count,
        },
        QcOutcome::Failed { message, summary } => JobOutcome::Failed {
            message: format!(
                "{message} (candidates={} errors={} warns={})",
                summary.candidate_count, summary.error_count, summary.warn_count
            ),
        },
    }
}
