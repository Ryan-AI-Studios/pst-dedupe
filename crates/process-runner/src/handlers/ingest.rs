//! Ingest handler: wraps `ingest-purview` on_job / resume APIs.

use ingest_purview::{ingest_path_on_job, resume_ingest, ExpandLimits};
use serde::Deserialize;

use crate::error::RunnerError;
use crate::handler::{JobContext, JobHandler, JobOutcome};

/// Params for kind `"ingest"`.
///
/// Start: `{ "path": "…" }`
/// Resume: checkpoint supplies `source_id`; optional `{ "source_id": "…" }`.
#[derive(Debug, Deserialize)]
struct IngestParams {
    path: Option<String>,
    source_id: Option<String>,
    #[serde(default)]
    limits: Option<IngestLimitsJson>,
}

#[derive(Debug, Deserialize)]
struct IngestLimitsJson {
    max_uncompressed_bytes: Option<u64>,
    max_entries: Option<u64>,
    checkpoint_every_n_entries: Option<u64>,
    /// Max single leaf size (streamed; multi-GB PSTs).
    max_entry_bytes: Option<u64>,
    /// Max full-buffer nested ZIP materialize size only.
    max_entry_buffer_bytes: Option<u64>,
}

/// Handler for package ingest (`ingest-purview`).
pub struct IngestHandler;

impl Default for IngestHandler {
    fn default() -> Self {
        Self
    }
}

impl IngestHandler {
    pub fn new() -> Self {
        Self
    }
}

impl JobHandler for IngestHandler {
    fn kind(&self) -> &'static str {
        "ingest"
    }

    fn run(&self, ctx: &JobContext<'_>) -> Result<JobOutcome, RunnerError> {
        let params: IngestParams = serde_json::from_str(ctx.params_json)
            .map_err(|e| RunnerError::InvalidParams(e.to_string()))?;
        let limits = limits_from(&params);
        let cancel_fn = ctx.cancel.as_fn();
        let cancel: Option<&dyn Fn() -> bool> = Some(&cancel_fn);

        ctx.progress.patch(|s| {
            s.stage = Some("expand".into());
            s.message = Some(if ctx.is_resume {
                "resume ingest".into()
            } else {
                "ingest".into()
            });
        });

        if ctx.is_resume {
            let source_id = params
                .source_id
                .as_deref()
                .or(ctx.source_id)
                .ok_or_else(|| {
                    RunnerError::InvalidParams(
                        "resume ingest requires source_id in params or context".into(),
                    )
                })?;
            let summary = resume_ingest(ctx.matter, source_id, ctx.job_id, &limits, cancel)
                .map_err(|e| RunnerError::HandlerFailed(e.to_string()))?;
            return Ok(summary_to_outcome(&summary));
        }

        let path = params.path.as_deref().ok_or_else(|| {
            RunnerError::InvalidParams("ingest start requires { \"path\": \"…\" }".into())
        })?;
        let path = camino::Utf8Path::new(path);
        let summary = ingest_path_on_job(ctx.matter, path, &limits, ctx.job_id, cancel)
            .map_err(|e| RunnerError::HandlerFailed(e.to_string()))?;
        Ok(summary_to_outcome(&summary))
    }
}

fn limits_from(params: &IngestParams) -> ExpandLimits {
    let mut limits = ExpandLimits::default();
    if let Some(ref j) = params.limits {
        if let Some(v) = j.max_uncompressed_bytes {
            limits.max_uncompressed_bytes = v;
        }
        if let Some(v) = j.max_entries {
            limits.max_entries = v;
        }
        if let Some(v) = j.checkpoint_every_n_entries {
            limits.checkpoint_every_n_entries = v;
        }
        if let Some(v) = j.max_entry_bytes {
            limits.max_entry_bytes = v;
        }
        if let Some(v) = j.max_entry_buffer_bytes {
            limits.max_entry_buffer_bytes = v;
        }
    }
    limits
}

fn summary_to_outcome(summary: &ingest_purview::IngestSummary) -> JobOutcome {
    if summary.cancelled {
        JobOutcome::Paused {
            message: Some("cancelled".into()),
            completed_count: summary.entries_ok,
        }
    } else if summary.completed {
        JobOutcome::Succeeded {
            message: Some(format!(
                "entries_ok={} psts={}",
                summary.entries_ok, summary.psts_found
            )),
            completed_count: summary.entries_ok,
        }
    } else {
        JobOutcome::Failed {
            message: format!(
                "ingest incomplete: entries_ok={} entries_err={}",
                summary.entries_ok, summary.entries_err
            ),
        }
    }
}
