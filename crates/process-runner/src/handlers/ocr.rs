//! Matter-level OCR handler (`ocr-plugin`).

use ocr_plugin::{run_ocr, OcrOutcome, OcrParams, JOB_KIND_OCR};

use crate::error::RunnerError;
use crate::handler::{JobContext, JobHandler, JobOutcome};

/// Handler for matter-level OCR (`kind = "ocr"`).
pub struct MatterOcrHandler;

impl Default for MatterOcrHandler {
    fn default() -> Self {
        Self
    }
}

impl MatterOcrHandler {
    pub fn new() -> Self {
        Self
    }
}

impl JobHandler for MatterOcrHandler {
    fn kind(&self) -> &'static str {
        JOB_KIND_OCR
    }

    fn run(&self, ctx: &JobContext<'_>) -> Result<JobOutcome, RunnerError> {
        let params = OcrParams::from_json(ctx.params_json)
            .map_err(|e| RunnerError::InvalidParams(e.to_string()))?;

        let cancel_fn = ctx.cancel.as_fn();
        let cancel: Option<&dyn Fn() -> bool> = Some(&cancel_fn);

        ctx.progress.patch(|s| {
            s.stage = Some("ocr".into());
            s.message = Some(if ctx.is_resume {
                "resume ocr".into()
            } else {
                "ocr".into()
            });
        });

        let progress_sink = ctx.progress.clone();
        let outcome = run_ocr(ctx.matter, ctx.job_id, &params, cancel, |completed| {
            progress_sink.patch(|s| {
                s.completed_count = completed;
                s.stage = Some("ocr".into());
            });
        })
        .map_err(|e| RunnerError::HandlerFailed(e.to_string()))?;

        Ok(map_outcome(outcome))
    }
}

fn map_outcome(outcome: OcrOutcome) -> JobOutcome {
    match outcome {
        OcrOutcome::Succeeded(s) => JobOutcome::Succeeded {
            message: Some(format!(
                "ocr={} skipped={} errors={}",
                s.ocr_count, s.skipped_count, s.error_count
            )),
            completed_count: s.completed_count,
        },
        OcrOutcome::Paused(s) => JobOutcome::Paused {
            message: Some("cancelled".into()),
            completed_count: s.completed_count,
        },
        OcrOutcome::Failed { message, summary } => JobOutcome::Failed {
            message: format!(
                "{message} (completed={} ocr={})",
                summary.completed_count, summary.ocr_count
            ),
        },
    }
}
