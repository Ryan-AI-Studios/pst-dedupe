//! Sequential processing-profile runner (`kind = "profile_run"`).
//!
//! Creates a **child job row per stage** and dispatches registered stage handlers
//! on the same worker thread. Never calls [`crate::ProcessRunner::start`] for children
//! (would hit Busy).

use std::collections::HashMap;
use std::sync::Arc;

use matter_core::{
    expand_profile_stage, profile_stage_plan, AuditEventInput, JobState, JOB_KIND_PROFILE_RUN,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::error::RunnerError;
use crate::handler::{JobContext, JobHandler, JobOutcome};

/// Handler for sequential profile execution.
///
/// Registers stage handlers that are available under the crate feature set.
pub struct MatterProfileRunHandler {
    stage_handlers: HashMap<String, Arc<dyn JobHandler>>,
}

impl Default for MatterProfileRunHandler {
    fn default() -> Self {
        Self::with_default_handlers()
    }
}

impl MatterProfileRunHandler {
    /// Empty handler map (tests can inject stages via [`Self::register_stage`]).
    pub fn new() -> Self {
        Self {
            stage_handlers: HashMap::new(),
        }
    }

    /// Register all allowlisted stage handlers enabled by crate features.
    pub fn with_default_handlers() -> Self {
        let mut h = Self::new();
        #[cfg(feature = "classify")]
        h.register_stage(Arc::new(crate::handlers::MatterClassifyHandler::new()));
        #[cfg(feature = "office")]
        h.register_stage(Arc::new(crate::handlers::MatterOfficeExtractHandler::new()));
        #[cfg(feature = "pdf")]
        h.register_stage(Arc::new(crate::handlers::MatterPdfExtractHandler::new()));
        #[cfg(feature = "calendar")]
        h.register_stage(Arc::new(crate::handlers::MatterIcsExtractHandler::new()));
        #[cfg(feature = "ocr")]
        h.register_stage(Arc::new(crate::handlers::MatterOcrHandler::new()));
        #[cfg(feature = "fts")]
        h.register_stage(Arc::new(crate::handlers::MatterFtsIndexHandler::new()));
        #[cfg(feature = "dedupe")]
        h.register_stage(Arc::new(crate::handlers::MatterDedupeHandler::new()));
        #[cfg(feature = "thread")]
        h.register_stage(Arc::new(crate::handlers::MatterThreadHandler::new()));
        #[cfg(feature = "neardup")]
        h.register_stage(Arc::new(crate::handlers::MatterNearDupHandler::new()));
        #[cfg(feature = "cull")]
        h.register_stage(Arc::new(crate::handlers::MatterCullHandler::new()));
        #[cfg(feature = "promote")]
        h.register_stage(Arc::new(crate::handlers::MatterPromoteHandler::new()));
        h
    }

    /// Register (or replace) a stage handler by its [`JobHandler::kind`].
    pub fn register_stage(&mut self, handler: Arc<dyn JobHandler>) {
        self.stage_handlers
            .insert(handler.kind().to_string(), handler);
    }
}

#[derive(Debug, Deserialize)]
struct ProfileRunParams {
    #[serde(default)]
    profile_id: Option<String>,
    #[serde(default)]
    profile_name: Option<String>,
    #[serde(default = "default_stop_on_failure")]
    stop_on_stage_failure: bool,
}

fn default_stop_on_failure() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StageCursorEntry {
    stage: String,
    job_id: String,
    status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProfileRunCursor {
    params: Value,
    stages: Vec<StageCursorEntry>,
    /// Resolved profile id frozen at first start (resume identity check).
    #[serde(default)]
    profile_id: Option<String>,
    /// Resolved profile name frozen at first start (resume identity check).
    #[serde(default)]
    profile_name: Option<String>,
}

impl JobHandler for MatterProfileRunHandler {
    fn kind(&self) -> &'static str {
        JOB_KIND_PROFILE_RUN
    }

    fn run(&self, ctx: &JobContext<'_>) -> Result<JobOutcome, RunnerError> {
        let params: ProfileRunParams = serde_json::from_str(ctx.params_json)
            .map_err(|e| RunnerError::InvalidParams(e.to_string()))?;

        let resolve_key = params
            .profile_id
            .as_deref()
            .or(params.profile_name.as_deref())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                RunnerError::InvalidParams("profile_run requires profile_id or profile_name".into())
            })?;

        let profile = ctx
            .matter
            .get_processing_profile(resolve_key)
            .map_err(|e| RunnerError::InvalidParams(e.to_string()))?;

        let plan = profile_stage_plan(&profile.body);
        if plan.is_empty() {
            return Err(RunnerError::InvalidParams(format!(
                "profile '{}' has no enabled stages",
                profile.name
            )));
        }

        // Load parent checkpoint for resume (fail closed on corrupt / IO error).
        let mut cursor =
            load_or_init_cursor(ctx, &params, ctx.params_json, &profile.id, &profile.name)?;

        let planned_kinds: Vec<String> = plan.iter().map(|s| s.kind.clone()).collect();
        audit_profile_run(
            ctx,
            "profile_run.start",
            &profile.id,
            &profile.name,
            &planned_kinds,
            None,
            None,
        )?;

        ctx.progress.patch(|s| {
            s.stage = Some("profile_run".into());
            s.message = Some(format!("profile {} ({} stages)", profile.name, plan.len()));
            s.total_hint = Some(plan.len() as u64);
            s.completed_count = cursor
                .stages
                .iter()
                .filter(|e| e.status == "succeeded")
                .count() as u64;
        });

        let stage_count = plan.len();
        let mut completed_stages: u64 = cursor
            .stages
            .iter()
            .filter(|e| e.status == "succeeded")
            .count() as u64;

        for (idx, stage) in plan.iter().enumerate() {
            // Skip already-succeeded stages (resume).
            if cursor
                .stages
                .iter()
                .any(|e| e.stage == stage.kind && e.status == "succeeded")
            {
                continue;
            }

            if ctx.cancel.is_cancelled() {
                persist_cursor(ctx, &cursor)?;
                audit_profile_run(
                    ctx,
                    "profile_run.paused",
                    &profile.id,
                    &profile.name,
                    &planned_kinds,
                    Some(&cursor.stages),
                    Some("cancelled"),
                )?;
                set_parent_if_running(ctx, JobState::Paused, Some("cancelled"))?;
                return Ok(JobOutcome::Paused {
                    message: Some("cancelled".into()),
                    completed_count: completed_stages,
                });
            }

            let handler = self.stage_handlers.get(&stage.kind).ok_or_else(|| {
                RunnerError::HandlerFailed(format!(
                    "no stage handler registered for kind '{}'",
                    stage.kind
                ))
            })?;

            let params_json = expand_profile_stage(&profile.body, &stage.kind)
                .map_err(|e| RunnerError::InvalidParams(e.to_string()))?;

            // Resume: reuse paused child job + checkpoint when present; never
            // ProcessRunner::start (Busy). Fresh stages get a new child row.
            let prior = cursor
                .stages
                .iter()
                .find(|e| e.stage == stage.kind && e.status == "paused")
                .cloned();
            let (child_id, is_child_resume) = if let Some(prev) = prior {
                let job = ctx
                    .matter
                    .get_job(&prev.job_id)
                    .map_err(RunnerError::from)?;
                if job.state == JobState::Paused {
                    ctx.matter
                        .set_job_state(&prev.job_id, JobState::Running, None)
                        .map_err(RunnerError::from)?;
                    (prev.job_id, true)
                } else if job.state == JobState::Failed {
                    // Failed stage with stop_on_failure=false may re-run; new child.
                    let child = ctx
                        .matter
                        .create_job_with_parent(&stage.kind, Some(ctx.job_id))
                        .map_err(RunnerError::from)?;
                    ctx.matter
                        .set_job_state(&child.id, JobState::Running, None)
                        .map_err(RunnerError::from)?;
                    (child.id, false)
                } else if job.state == JobState::Running {
                    // Orphan Running after crash — continue as resume.
                    (prev.job_id, true)
                } else {
                    let child = ctx
                        .matter
                        .create_job_with_parent(&stage.kind, Some(ctx.job_id))
                        .map_err(RunnerError::from)?;
                    ctx.matter
                        .set_job_state(&child.id, JobState::Running, None)
                        .map_err(RunnerError::from)?;
                    (child.id, false)
                }
            } else {
                let child = ctx
                    .matter
                    .create_job_with_parent(&stage.kind, Some(ctx.job_id))
                    .map_err(RunnerError::from)?;
                ctx.matter
                    .set_job_state(&child.id, JobState::Running, None)
                    .map_err(RunnerError::from)?;
                (child.id, false)
            };

            cursor.stages.retain(|e| e.stage != stage.kind);
            cursor.stages.push(StageCursorEntry {
                stage: stage.kind.clone(),
                job_id: child_id.clone(),
                status: "running".into(),
            });
            persist_cursor(ctx, &cursor)?;

            ctx.progress.patch(|s| {
                s.stage = Some(stage.kind.clone());
                s.message = Some(format!(
                    "stage {}/{}: {} (child {}){}",
                    idx + 1,
                    stage_count,
                    stage.kind,
                    child_id,
                    if is_child_resume { " resume" } else { "" }
                ));
                s.completed_count = completed_stages;
                s.total_hint = Some(stage_count as u64);
            });

            let child_ctx = JobContext {
                matter: ctx.matter,
                job_id: &child_id,
                source_id: None,
                params_json: &params_json,
                cancel: ctx.cancel,
                progress: ctx.progress.clone(),
                is_resume: is_child_resume,
            };

            let child_outcome = handler.run(&child_ctx);
            let terminal = finalize_child(ctx, &child_id, child_outcome);

            // Update checkpoint entry.
            if let Some(entry) = cursor.stages.iter_mut().find(|e| e.job_id == child_id) {
                entry.status = terminal.status_str().to_string();
            }
            persist_cursor(ctx, &cursor)?;

            match terminal {
                ChildTerminal::Succeeded { .. } => {
                    completed_stages += 1;
                    ctx.progress.patch(|s| {
                        s.completed_count = completed_stages;
                    });
                }
                ChildTerminal::Paused {
                    message,
                    completed_count,
                } => {
                    audit_profile_run(
                        ctx,
                        "profile_run.paused",
                        &profile.id,
                        &profile.name,
                        &planned_kinds,
                        Some(&cursor.stages),
                        message.as_deref(),
                    )?;
                    set_parent_if_running(ctx, JobState::Paused, message.as_deref())?;
                    return Ok(JobOutcome::Paused {
                        message,
                        completed_count: completed_stages.max(completed_count),
                    });
                }
                ChildTerminal::Failed { message } => {
                    if params.stop_on_stage_failure {
                        audit_profile_run(
                            ctx,
                            "profile_run.failed",
                            &profile.id,
                            &profile.name,
                            &planned_kinds,
                            Some(&cursor.stages),
                            Some(&message),
                        )?;
                        set_parent_if_running(ctx, JobState::Failed, Some(&message))?;
                        return Ok(JobOutcome::Failed {
                            message: format!("stage {} failed: {message}", stage.kind),
                        });
                    }
                    // Continue on failure when configured.
                    ctx.progress.patch(|s| {
                        s.message = Some(format!(
                            "stage {} failed (continuing): {message}",
                            stage.kind
                        ));
                    });
                }
            }
        }

        let msg = format!(
            "profile {} complete: {} stages",
            profile.name, completed_stages
        );
        audit_profile_run(
            ctx,
            "profile_run.complete",
            &profile.id,
            &profile.name,
            &planned_kinds,
            Some(&cursor.stages),
            Some(&msg),
        )?;
        set_parent_if_running(ctx, JobState::Succeeded, None)?;

        Ok(JobOutcome::Succeeded {
            message: Some(msg),
            completed_count: completed_stages,
        })
    }
}

enum ChildTerminal {
    Succeeded {
        #[allow(dead_code)]
        completed_count: u64,
    },
    Paused {
        message: Option<String>,
        completed_count: u64,
    },
    Failed {
        message: String,
    },
}

impl ChildTerminal {
    fn status_str(&self) -> &'static str {
        match self {
            ChildTerminal::Succeeded { .. } => "succeeded",
            ChildTerminal::Paused { .. } => "paused",
            ChildTerminal::Failed { .. } => "failed",
        }
    }
}

/// Map child handler result to a terminal state, setting job state if still Running.
fn finalize_child(
    ctx: &JobContext<'_>,
    child_job_id: &str,
    outcome: Result<JobOutcome, RunnerError>,
) -> ChildTerminal {
    let durable = ctx.matter.get_job(child_job_id).ok();
    let still_running = durable
        .as_ref()
        .map(|j| j.state == JobState::Running)
        .unwrap_or(true);

    match outcome {
        Ok(JobOutcome::Succeeded {
            message: _,
            completed_count,
        }) => {
            if still_running {
                let _ = ctx
                    .matter
                    .set_job_state(child_job_id, JobState::Succeeded, None);
            }
            ChildTerminal::Succeeded { completed_count }
        }
        Ok(JobOutcome::Paused {
            message,
            completed_count,
        }) => {
            if still_running {
                let summary = message.as_deref().unwrap_or("paused");
                let _ = ctx
                    .matter
                    .set_job_state(child_job_id, JobState::Paused, Some(summary));
            }
            ChildTerminal::Paused {
                message,
                completed_count,
            }
        }
        Ok(JobOutcome::Failed { message }) => {
            if still_running {
                let _ = ctx
                    .matter
                    .set_job_state(child_job_id, JobState::Failed, Some(&message));
            }
            ChildTerminal::Failed { message }
        }
        Err(e) => {
            let message = e.to_string();
            let _ = ctx
                .matter
                .set_job_state(child_job_id, JobState::Failed, Some(&message));
            ChildTerminal::Failed { message }
        }
    }
}

fn set_parent_if_running(
    ctx: &JobContext<'_>,
    state: JobState,
    summary: Option<&str>,
) -> Result<(), RunnerError> {
    if let Ok(job) = ctx.matter.get_job(ctx.job_id) {
        if job.state == JobState::Running {
            ctx.matter
                .set_job_state(ctx.job_id, state, summary)
                .map_err(RunnerError::from)?;
        }
    }
    Ok(())
}

fn load_or_init_cursor(
    ctx: &JobContext<'_>,
    params: &ProfileRunParams,
    raw_params: &str,
    resolved_profile_id: &str,
    resolved_profile_name: &str,
) -> Result<ProfileRunCursor, RunnerError> {
    match ctx.matter.get_checkpoint(ctx.job_id, "profile_run") {
        Err(e) => Err(RunnerError::from(e)),
        Ok(Some(cp)) => {
            // Fail closed: corrupt checkpoint must not silently restart empty.
            let cursor: ProfileRunCursor = serde_json::from_str(&cp.cursor_json)
                .map_err(|e| RunnerError::Other(format!("corrupt profile_run checkpoint: {e}")))?;
            if let Some(ref id) = cursor.profile_id {
                if id != resolved_profile_id {
                    return Err(RunnerError::InvalidParams(format!(
                        "profile_run resume profile_id mismatch: checkpoint '{id}' vs resolved '{resolved_profile_id}'"
                    )));
                }
            }
            if let Some(ref name) = cursor.profile_name {
                if name != resolved_profile_name {
                    return Err(RunnerError::InvalidParams(format!(
                        "profile_run resume profile_name mismatch: checkpoint '{name}' vs resolved '{resolved_profile_name}'"
                    )));
                }
            }
            Ok(cursor)
        }
        Ok(None) => {
            // Fresh start only when no checkpoint exists.
            let params_val: Value = serde_json::from_str(raw_params).unwrap_or_else(|_| {
                json!({
                    "profile_id": params.profile_id,
                    "profile_name": params.profile_name,
                    "stop_on_stage_failure": params.stop_on_stage_failure,
                })
            });
            Ok(ProfileRunCursor {
                params: params_val,
                stages: Vec::new(),
                profile_id: Some(resolved_profile_id.to_string()),
                profile_name: Some(resolved_profile_name.to_string()),
            })
        }
    }
}

fn persist_cursor(ctx: &JobContext<'_>, cursor: &ProfileRunCursor) -> Result<(), RunnerError> {
    let json = serde_json::to_string(cursor)
        .map_err(|e| RunnerError::Other(format!("serialize profile_run cursor: {e}")))?;
    let completed = cursor
        .stages
        .iter()
        .filter(|e| e.status == "succeeded")
        .count() as i64;
    ctx.matter
        .put_checkpoint(ctx.job_id, "profile_run", &json, completed)
        .map_err(RunnerError::from)?;
    Ok(())
}

/// Append a profile_run audit event. Failures propagate so the job fails closed.
fn audit_profile_run(
    ctx: &JobContext<'_>,
    action: &str,
    profile_id: &str,
    profile_name: &str,
    planned_kinds: &[String],
    stage_outcomes: Option<&[StageCursorEntry]>,
    message: Option<&str>,
) -> Result<(), RunnerError> {
    let mut params = json!({
        "profile_id": profile_id,
        "profile_name": profile_name,
        "stages": planned_kinds,
        "message": message,
        "parent_job_id": ctx.job_id,
    });
    if let Some(outcomes) = stage_outcomes {
        let outcomes_val = serde_json::to_value(outcomes).map_err(|e| {
            RunnerError::Other(format!("serialize profile_run stage_outcomes: {e}"))
        })?;
        if let Some(obj) = params.as_object_mut() {
            obj.insert("stage_outcomes".into(), outcomes_val);
        }
    }
    ctx.matter
        .append_audit(AuditEventInput {
            actor: "system".into(),
            action: action.into(),
            entity: format!("job:{}", ctx.job_id),
            params_json: params.to_string(),
            tool_version: env!("CARGO_PKG_VERSION").into(),
        })
        .map_err(RunnerError::from)?;
    Ok(())
}
