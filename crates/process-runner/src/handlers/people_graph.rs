//! Matter-level people–comms graph handler (`matter-people`).

use matter_people::{
    run_people_graph, PeopleGraphOutcome, PeopleGraphParams, JOB_KIND_PEOPLE_GRAPH,
};

use crate::error::RunnerError;
use crate::handler::{JobContext, JobHandler, JobOutcome};

/// Handler for matter-level people graph (`kind = "people_graph"`).
pub struct MatterPeopleGraphHandler;

impl Default for MatterPeopleGraphHandler {
    fn default() -> Self {
        Self
    }
}

impl MatterPeopleGraphHandler {
    pub fn new() -> Self {
        Self
    }
}

impl JobHandler for MatterPeopleGraphHandler {
    fn kind(&self) -> &'static str {
        JOB_KIND_PEOPLE_GRAPH
    }

    fn run(&self, ctx: &JobContext<'_>) -> Result<JobOutcome, RunnerError> {
        let params = PeopleGraphParams::from_json(ctx.params_json)
            .map_err(|e| RunnerError::InvalidParams(e.to_string()))?;

        let cancel_fn = ctx.cancel.as_fn();
        let cancel: Option<&dyn Fn() -> bool> = Some(&cancel_fn);

        ctx.progress.patch(|s| {
            s.stage = Some("people_graph".into());
            s.message = Some(if ctx.is_resume {
                "resume people_graph".into()
            } else {
                "people_graph".into()
            });
        });

        let progress_sink = ctx.progress.clone();
        let outcome = run_people_graph(ctx.matter, ctx.job_id, &params, cancel, |completed| {
            progress_sink.patch(|s| {
                s.completed_count = completed;
                s.stage = Some("people_graph".into());
            });
        })
        .map_err(|e| RunnerError::HandlerFailed(e.to_string()))?;

        Ok(map_outcome(outcome))
    }
}

fn map_outcome(outcome: PeopleGraphOutcome) -> JobOutcome {
    match outcome {
        PeopleGraphOutcome::Succeeded(r) => JobOutcome::Succeeded {
            message: Some(format!(
                "people_graph items={} people={} edges={} participants={}",
                r.items_processed, r.people_count, r.edge_count, r.participants_written
            )),
            completed_count: r.items_processed,
        },
        PeopleGraphOutcome::Paused(s) => JobOutcome::Paused {
            message: Some("cancelled".into()),
            completed_count: s.completed_count,
        },
        PeopleGraphOutcome::Failed { message, summary } => JobOutcome::Failed {
            message: format!(
                "{message} (items={} people={} edges={})",
                summary.items_processed, summary.people_count, summary.edge_count
            ),
        },
    }
}
