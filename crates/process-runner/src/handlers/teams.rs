//! Matter-level Teams/chat extract handler (`extract-teams`).

use extract_teams::{
    run_teams_extract, TeamsExtractOutcome, TeamsExtractParams, JOB_KIND_TEAMS_EXTRACT,
};

use crate::error::RunnerError;
use crate::handler::{JobContext, JobHandler, JobOutcome};

/// Handler for matter-level Teams/chat extract (`kind = "teams_extract"`).
pub struct MatterTeamsExtractHandler;

impl Default for MatterTeamsExtractHandler {
    fn default() -> Self {
        Self
    }
}

impl MatterTeamsExtractHandler {
    pub fn new() -> Self {
        Self
    }
}

impl JobHandler for MatterTeamsExtractHandler {
    fn kind(&self) -> &'static str {
        JOB_KIND_TEAMS_EXTRACT
    }

    fn run(&self, ctx: &JobContext<'_>) -> Result<JobOutcome, RunnerError> {
        let params = TeamsExtractParams::from_json(ctx.params_json)
            .map_err(|e| RunnerError::InvalidParams(e.to_string()))?;

        let cancel_fn = ctx.cancel.as_fn();
        let cancel: Option<&dyn Fn() -> bool> = Some(&cancel_fn);

        ctx.progress.patch(|s| {
            s.stage = Some("teams_extract".into());
            s.message = Some(if ctx.is_resume {
                "resume teams_extract".into()
            } else {
                "teams_extract".into()
            });
        });

        let progress_sink = ctx.progress.clone();
        let outcome = run_teams_extract(ctx.matter, ctx.job_id, &params, cancel, |completed| {
            progress_sink.patch(|s| {
                s.completed_count = completed;
                s.stage = Some("teams_extract".into());
            });
        })
        .map_err(|e| RunnerError::HandlerFailed(e.to_string()))?;

        Ok(map_outcome(outcome))
    }
}

fn map_outcome(outcome: TeamsExtractOutcome) -> JobOutcome {
    match outcome {
        TeamsExtractOutcome::Succeeded(s) => JobOutcome::Succeeded {
            message: Some(format!(
                "extracted={} skipped={} errors={} children={}",
                s.extracted_count, s.skipped_count, s.error_count, s.child_count
            )),
            completed_count: s.completed_count,
        },
        TeamsExtractOutcome::Paused(s) => JobOutcome::Paused {
            message: Some("cancelled".into()),
            completed_count: s.completed_count,
        },
        TeamsExtractOutcome::Failed { message, summary } => JobOutcome::Failed {
            message: format!(
                "{message} (completed={} extracted={})",
                summary.completed_count, summary.extracted_count
            ),
        },
    }
}
