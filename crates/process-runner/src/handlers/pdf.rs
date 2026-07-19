//! Matter-level PDF extract handler (`extract-pdf`).

use extract_pdf::{run_pdf_extract, PdfExtractOutcome, PdfExtractParams, JOB_KIND_PDF_EXTRACT};

use crate::error::RunnerError;
use crate::handler::{JobContext, JobHandler, JobOutcome};

/// Handler for matter-level PDF extract (`kind = "pdf_extract"`).
pub struct MatterPdfExtractHandler;

impl Default for MatterPdfExtractHandler {
    fn default() -> Self {
        Self
    }
}

impl MatterPdfExtractHandler {
    pub fn new() -> Self {
        Self
    }
}

impl JobHandler for MatterPdfExtractHandler {
    fn kind(&self) -> &'static str {
        JOB_KIND_PDF_EXTRACT
    }

    fn run(&self, ctx: &JobContext<'_>) -> Result<JobOutcome, RunnerError> {
        let params = PdfExtractParams::from_json(ctx.params_json)
            .map_err(|e| RunnerError::InvalidParams(e.to_string()))?;

        let cancel_fn = ctx.cancel.as_fn();
        let cancel: Option<&dyn Fn() -> bool> = Some(&cancel_fn);

        ctx.progress.patch(|s| {
            s.stage = Some("pdf_extract".into());
            s.message = Some(if ctx.is_resume {
                "resume pdf_extract".into()
            } else {
                "pdf_extract".into()
            });
        });

        let progress_sink = ctx.progress.clone();
        let outcome = run_pdf_extract(ctx.matter, ctx.job_id, &params, cancel, |completed| {
            progress_sink.patch(|s| {
                s.completed_count = completed;
                s.stage = Some("pdf_extract".into());
            });
        })
        .map_err(|e| RunnerError::HandlerFailed(e.to_string()))?;

        Ok(map_outcome(outcome))
    }
}

fn map_outcome(outcome: PdfExtractOutcome) -> JobOutcome {
    match outcome {
        PdfExtractOutcome::Succeeded(s) => JobOutcome::Succeeded {
            message: Some(format!(
                "extracted={} skipped={} errors={}",
                s.extracted_count, s.skipped_count, s.error_count
            )),
            completed_count: s.completed_count,
        },
        PdfExtractOutcome::Paused(s) => JobOutcome::Paused {
            message: Some("cancelled".into()),
            completed_count: s.completed_count,
        },
        PdfExtractOutcome::Failed { message, summary } => JobOutcome::Failed {
            message: format!(
                "{message} (completed={} extracted={})",
                summary.completed_count, summary.extracted_count
            ),
        },
    }
}
