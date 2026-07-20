//! Matter-level concept clustering handler (`matter-cluster`).

use matter_cluster::{
    run_concept_cluster, ConceptClusterOutcome, ConceptClusterParams, JOB_KIND_CONCEPT_CLUSTER,
};

use crate::error::RunnerError;
use crate::handler::{JobContext, JobHandler, JobOutcome};

/// Handler for matter-level concept clustering (`kind = "concept_cluster"`).
pub struct MatterConceptClusterHandler;

impl Default for MatterConceptClusterHandler {
    fn default() -> Self {
        Self
    }
}

impl MatterConceptClusterHandler {
    pub fn new() -> Self {
        Self
    }
}

impl JobHandler for MatterConceptClusterHandler {
    fn kind(&self) -> &'static str {
        JOB_KIND_CONCEPT_CLUSTER
    }

    fn run(&self, ctx: &JobContext<'_>) -> Result<JobOutcome, RunnerError> {
        let params = ConceptClusterParams::from_json(ctx.params_json)
            .map_err(|e| RunnerError::InvalidParams(e.to_string()))?;

        let cancel_fn = ctx.cancel.as_fn();
        let cancel: Option<&dyn Fn() -> bool> = Some(&cancel_fn);

        ctx.progress.patch(|s| {
            s.stage = Some("concept_cluster".into());
            s.message = Some(if ctx.is_resume {
                "resume concept_cluster".into()
            } else {
                "concept_cluster".into()
            });
        });

        let progress_sink = ctx.progress.clone();
        let outcome = run_concept_cluster(ctx.matter, ctx.job_id, &params, cancel, |completed| {
            progress_sink.patch(|s| {
                s.completed_count = completed;
                s.stage = Some("concept_cluster".into());
            });
        })
        .map_err(|e| RunnerError::HandlerFailed(e.to_string()))?;

        Ok(map_outcome(outcome))
    }
}

fn map_outcome(outcome: ConceptClusterOutcome) -> JobOutcome {
    match outcome {
        ConceptClusterOutcome::Succeeded(r) => JobOutcome::Succeeded {
            message: Some(format!(
                "concept_cluster clustered={} clusters={} (k={}) method={}",
                r.clustered_count, r.cluster_count, r.k_requested, r.method
            )),
            completed_count: r.clustered_count,
        },
        ConceptClusterOutcome::Paused(s) => JobOutcome::Paused {
            message: Some("cancelled".into()),
            completed_count: s.completed_count,
        },
        ConceptClusterOutcome::Failed { message, summary } => JobOutcome::Failed {
            message: format!(
                "{message} (candidates={} clusters={})",
                summary.candidate_count, summary.cluster_count
            ),
        },
    }
}
