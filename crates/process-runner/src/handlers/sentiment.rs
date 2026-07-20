//! Matter-level offline sentiment handler (`matter-sentiment`).

use matter_sentiment::{run_sentiment, SentimentOutcome, SentimentParams, JOB_KIND_SENTIMENT};

use crate::error::RunnerError;
use crate::handler::{JobContext, JobHandler, JobOutcome};

/// Handler for matter-level sentiment (`kind = "sentiment"`).
pub struct MatterSentimentHandler;

impl Default for MatterSentimentHandler {
    fn default() -> Self {
        Self
    }
}

impl MatterSentimentHandler {
    pub fn new() -> Self {
        Self
    }
}

impl JobHandler for MatterSentimentHandler {
    fn kind(&self) -> &'static str {
        JOB_KIND_SENTIMENT
    }

    fn run(&self, ctx: &JobContext<'_>) -> Result<JobOutcome, RunnerError> {
        let params = SentimentParams::from_json(ctx.params_json)
            .map_err(|e| RunnerError::InvalidParams(e.to_string()))?;

        let cancel_fn = ctx.cancel.as_fn();
        let cancel: Option<&dyn Fn() -> bool> = Some(&cancel_fn);

        ctx.progress.patch(|s| {
            s.stage = Some("sentiment".into());
            s.message = Some(if ctx.is_resume {
                "resume sentiment".into()
            } else {
                "sentiment".into()
            });
        });

        let progress_sink = ctx.progress.clone();
        let outcome = run_sentiment(ctx.matter, ctx.job_id, &params, cancel, |completed| {
            progress_sink.patch(|s| {
                s.completed_count = completed;
                s.stage = Some("sentiment".into());
            });
        })
        .map_err(|e| RunnerError::HandlerFailed(e.to_string()))?;

        Ok(map_outcome(outcome))
    }
}

fn map_outcome(outcome: SentimentOutcome) -> JobOutcome {
    match outcome {
        SentimentOutcome::Succeeded(r) => JobOutcome::Succeeded {
            message: Some(format!(
                "sentiment scanned={} skipped={} relabeled={} unscored={} errors={}",
                r.scanned_count,
                r.skipped_count,
                r.relabeled_count,
                r.unscored_count,
                r.error_count
            )),
            completed_count: r.completed_count,
        },
        SentimentOutcome::Paused(s) => JobOutcome::Paused {
            message: Some("cancelled".into()),
            completed_count: s.completed_count,
        },
        SentimentOutcome::Failed { message, summary } => JobOutcome::Failed {
            message: format!(
                "{message} (scanned={} relabeled={} errors={})",
                summary.scanned_count, summary.relabeled_count, summary.error_count
            ),
        },
    }
}
