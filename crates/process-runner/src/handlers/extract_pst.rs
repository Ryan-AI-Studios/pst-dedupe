//! Extract-PST handler: wraps `extract-pst` on_job / resume APIs.

use extract_pst::{
    extract_pst_item_on_job, extract_pst_path_on_job, resume_extract, ExtractLimits,
    JOB_KIND_EXTRACT_PST,
};
use serde::Deserialize;

use crate::error::RunnerError;
use crate::handler::{JobContext, JobHandler, JobOutcome};

/// Params for kind `"extract_pst"`.
///
/// Start (inventory): `{ "source_id", "pst_item_id" }`
/// Start (path): `{ "source_id", "path" }`
/// Resume: `{ "source_id" }` (or from checkpoint).
#[derive(Debug, Deserialize)]
struct ExtractParams {
    source_id: Option<String>,
    pst_item_id: Option<String>,
    path: Option<String>,
    #[serde(default)]
    limits: Option<ExtractLimitsJson>,
}

#[derive(Debug, Deserialize)]
struct ExtractLimitsJson {
    batch_size: Option<u64>,
    max_messages: Option<u64>,
}

/// Handler for PST extract (`extract-pst`).
pub struct ExtractPstHandler;

impl Default for ExtractPstHandler {
    fn default() -> Self {
        Self
    }
}

impl ExtractPstHandler {
    pub fn new() -> Self {
        Self
    }
}

impl JobHandler for ExtractPstHandler {
    fn kind(&self) -> &'static str {
        JOB_KIND_EXTRACT_PST
    }

    fn run(&self, ctx: &JobContext<'_>) -> Result<JobOutcome, RunnerError> {
        let params: ExtractParams = serde_json::from_str(ctx.params_json)
            .map_err(|e| RunnerError::InvalidParams(e.to_string()))?;
        let limits = limits_from(&params);
        let cancel_fn = ctx.cancel.as_fn();
        let cancel: Option<&dyn Fn() -> bool> = Some(&cancel_fn);

        ctx.progress.patch(|s| {
            s.stage = Some("pst_extract".into());
            s.message = Some(if ctx.is_resume {
                "resume extract".into()
            } else {
                "extract_pst".into()
            });
        });

        let source_id = params
            .source_id
            .as_deref()
            .or(ctx.source_id)
            .ok_or_else(|| {
                RunnerError::InvalidParams(
                    "extract_pst requires source_id in params or context".into(),
                )
            })?;

        if ctx.is_resume {
            let summary = resume_extract(ctx.matter, source_id, ctx.job_id, &limits, cancel)
                .map_err(|e| RunnerError::HandlerFailed(e.to_string()))?;
            return Ok(summary_to_outcome(&summary));
        }

        if let Some(ref pst_item_id) = params.pst_item_id {
            let summary = extract_pst_item_on_job(
                ctx.matter,
                source_id,
                pst_item_id,
                &limits,
                ctx.job_id,
                cancel,
            )
            .map_err(|e| RunnerError::HandlerFailed(e.to_string()))?;
            return Ok(summary_to_outcome(&summary));
        }

        if let Some(ref path) = params.path {
            let summary =
                extract_pst_path_on_job(ctx.matter, source_id, path, &limits, ctx.job_id, cancel)
                    .map_err(|e| RunnerError::HandlerFailed(e.to_string()))?;
            return Ok(summary_to_outcome(&summary));
        }

        Err(RunnerError::InvalidParams(
            "extract_pst start requires pst_item_id or path".into(),
        ))
    }
}

fn limits_from(params: &ExtractParams) -> ExtractLimits {
    let mut limits = ExtractLimits::default();
    if let Some(ref j) = params.limits {
        if let Some(v) = j.batch_size {
            limits.batch_size = v;
        }
        if let Some(v) = j.max_messages {
            limits.max_messages = Some(v);
        }
    }
    limits
}

fn summary_to_outcome(summary: &extract_pst::ExtractSummary) -> JobOutcome {
    if summary.cancelled {
        JobOutcome::Paused {
            message: Some("cancelled".into()),
            completed_count: summary.messages_ok,
        }
    } else if summary.completed {
        JobOutcome::Succeeded {
            message: Some(format!(
                "messages_ok={} messages_err={}",
                summary.messages_ok, summary.messages_err
            )),
            completed_count: summary.messages_ok,
        }
    } else {
        // max_messages pause or incomplete walk — durable state already Paused.
        JobOutcome::Paused {
            message: Some("incomplete".into()),
            completed_count: summary.messages_ok,
        }
    }
}
