//! Matter-level first-pass AI code suggestions handler (`matter-ai`).

use matter_ai::{
    run_ai_suggest_codes, AiSuggestCodesParams, AiSuggestOutcome, JOB_KIND_AI_SUGGEST_CODES,
};

use crate::error::RunnerError;
use crate::handler::{JobContext, JobHandler, JobOutcome};

/// Handler for matter-level AI code suggestions (`kind = "ai_suggest_codes"`).
pub struct MatterAiSuggestCodesHandler;

impl Default for MatterAiSuggestCodesHandler {
    fn default() -> Self {
        Self
    }
}

impl MatterAiSuggestCodesHandler {
    pub fn new() -> Self {
        Self
    }
}

impl JobHandler for MatterAiSuggestCodesHandler {
    fn kind(&self) -> &'static str {
        JOB_KIND_AI_SUGGEST_CODES
    }

    fn run(&self, ctx: &JobContext<'_>) -> Result<JobOutcome, RunnerError> {
        let params = AiSuggestCodesParams::from_json(ctx.params_json)
            .map_err(|e| RunnerError::InvalidParams(e.to_string()))?;

        let cancel_fn = ctx.cancel.as_fn();
        let cancel: Option<&dyn Fn() -> bool> = Some(&cancel_fn);

        ctx.progress.patch(|s| {
            s.stage = Some("ai_suggest_codes".into());
            s.message = Some(if ctx.is_resume {
                "resume ai_suggest_codes".into()
            } else {
                "ai_suggest_codes".into()
            });
        });

        let progress_sink = ctx.progress.clone();
        let outcome = run_ai_suggest_codes(ctx.matter, ctx.job_id, &params, cancel, |completed| {
            progress_sink.patch(|s| {
                s.completed_count = completed;
                s.stage = Some("ai_suggest_codes".into());
            });
        })
        .map_err(|e| RunnerError::HandlerFailed(e.to_string()))?;

        Ok(map_outcome(outcome))
    }
}

fn map_outcome(outcome: AiSuggestOutcome) -> JobOutcome {
    match outcome {
        AiSuggestOutcome::Succeeded(r) => JobOutcome::Succeeded {
            message: Some(format!(
                "ai_suggest_codes suggested={} skipped={} withheld={} errors={} rows={} model={} remote={}",
                r.suggested_count,
                r.skipped_count,
                r.withheld_count,
                r.error_count,
                r.suggestion_rows,
                r.model,
                r.is_remote
            )),
            completed_count: r.completed_count,
        },
        AiSuggestOutcome::Paused(s) => JobOutcome::Paused {
            message: Some("cancelled".into()),
            completed_count: s.completed_count,
        },
        AiSuggestOutcome::Failed { message, summary } => JobOutcome::Failed {
            message: format!(
                "{message} (suggested={} skipped={} errors={})",
                summary.suggested_count, summary.skipped_count, summary.error_count
            ),
        },
    }
}
