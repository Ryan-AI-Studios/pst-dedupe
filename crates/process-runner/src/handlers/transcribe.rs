//! Matter-level STT / transcription handler (`stt-plugin`).

use stt_plugin::{run_transcribe, SttOutcome, SttParams, JOB_KIND_TRANSCRIBE};

use crate::error::RunnerError;
use crate::handler::{JobContext, JobHandler, JobOutcome};

/// Handler for matter-level transcription (`kind = "transcribe"`).
pub struct MatterTranscribeHandler;

impl Default for MatterTranscribeHandler {
    fn default() -> Self {
        Self
    }
}

impl MatterTranscribeHandler {
    pub fn new() -> Self {
        Self
    }
}

impl JobHandler for MatterTranscribeHandler {
    fn kind(&self) -> &'static str {
        JOB_KIND_TRANSCRIBE
    }

    fn run(&self, ctx: &JobContext<'_>) -> Result<JobOutcome, RunnerError> {
        let params = SttParams::from_json(ctx.params_json)
            .map_err(|e| RunnerError::InvalidParams(e.to_string()))?;

        let cancel_fn = ctx.cancel.as_fn();
        let cancel: Option<&dyn Fn() -> bool> = Some(&cancel_fn);

        ctx.progress.patch(|s| {
            s.stage = Some("transcribe".into());
            s.message = Some(if ctx.is_resume {
                "resume transcribe".into()
            } else {
                "transcribe".into()
            });
        });

        let progress_sink = ctx.progress.clone();
        let outcome = run_transcribe(ctx.matter, ctx.job_id, &params, cancel, |completed| {
            progress_sink.patch(|s| {
                s.completed_count = completed;
                s.stage = Some("transcribe".into());
            });
        })
        .map_err(|e| RunnerError::HandlerFailed(e.to_string()))?;

        Ok(map_outcome(outcome))
    }
}

fn map_outcome(outcome: SttOutcome) -> JobOutcome {
    match outcome {
        SttOutcome::Succeeded(s) => JobOutcome::Succeeded {
            message: Some(format!(
                "transcripts={} skipped={} errors={}",
                s.transcript_count, s.skipped_count, s.error_count
            )),
            completed_count: s.completed_count,
        },
        SttOutcome::Paused(s) => JobOutcome::Paused {
            message: Some("cancelled".into()),
            completed_count: s.completed_count,
        },
        SttOutcome::Failed { message, summary } => JobOutcome::Failed {
            message: format!(
                "{message} (completed={} transcripts={})",
                summary.completed_count, summary.transcript_count
            ),
        },
    }
}
