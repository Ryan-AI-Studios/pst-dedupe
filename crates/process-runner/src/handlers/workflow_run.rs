//! Sequential workflow runner (`kind = "workflow_run"`).
//!
//! Creates a **child job row per node** (job / profile_run / gate) and dispatches
//! registered node handlers on the **same worker thread**. Never calls
//! [`crate::ProcessRunner::start`] for children (would hit Busy).
//!
//! Nested `profile_run` nodes receive `parent_job_id = workflow_run`; their stage
//! children parent to the profile_run job (via `create_job_with_parent` retrofit).

use std::collections::HashMap;
use std::sync::Arc;

use matter_core::{
    bind_workflow, parse_workflow_body, workflow_definition_hash, AuditEventInput, JobState,
    WorkflowBody, WorkflowNodeType, JOB_KIND_PROFILE_RUN, JOB_KIND_WORKFLOW_RUN,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::error::RunnerError;
use crate::handler::{JobContext, JobHandler, JobOutcome};
use crate::handlers::MatterProfileRunHandler;

/// Handler for sequential workflow execution.
///
/// Registers node handlers available under the crate feature set, plus nested
/// [`MatterProfileRunHandler`] for `type=profile_run` nodes.
pub struct MatterWorkflowRunHandler {
    node_handlers: HashMap<String, Arc<dyn JobHandler>>,
}

impl Default for MatterWorkflowRunHandler {
    fn default() -> Self {
        Self::with_default_handlers()
    }
}

impl MatterWorkflowRunHandler {
    /// Empty handler map (tests inject nodes via [`Self::register_node`]).
    pub fn new() -> Self {
        Self {
            node_handlers: HashMap::new(),
        }
    }

    /// Register all allowlisted node handlers enabled by crate features.
    pub fn with_default_handlers() -> Self {
        let mut h = Self::new();
        #[cfg(feature = "ingest")]
        h.register_node(Arc::new(crate::handlers::IngestHandler::new()));
        #[cfg(feature = "extract_pst")]
        h.register_node(Arc::new(crate::handlers::ExtractPstHandler::new()));
        #[cfg(feature = "classify")]
        h.register_node(Arc::new(crate::handlers::MatterClassifyHandler::new()));
        #[cfg(feature = "office")]
        h.register_node(Arc::new(crate::handlers::MatterOfficeExtractHandler::new()));
        #[cfg(feature = "pdf")]
        h.register_node(Arc::new(crate::handlers::MatterPdfExtractHandler::new()));
        #[cfg(feature = "calendar")]
        h.register_node(Arc::new(crate::handlers::MatterIcsExtractHandler::new()));
        #[cfg(feature = "ocr")]
        h.register_node(Arc::new(crate::handlers::MatterOcrHandler::new()));
        #[cfg(feature = "fts")]
        h.register_node(Arc::new(crate::handlers::MatterFtsIndexHandler::new()));
        #[cfg(feature = "dedupe")]
        h.register_node(Arc::new(crate::handlers::MatterDedupeHandler::new()));
        #[cfg(feature = "thread")]
        h.register_node(Arc::new(crate::handlers::MatterThreadHandler::new()));
        #[cfg(feature = "neardup")]
        h.register_node(Arc::new(crate::handlers::MatterNearDupHandler::new()));
        #[cfg(feature = "cull")]
        h.register_node(Arc::new(crate::handlers::MatterCullHandler::new()));
        #[cfg(feature = "promote")]
        h.register_node(Arc::new(crate::handlers::MatterPromoteHandler::new()));
        #[cfg(feature = "qc")]
        h.register_node(Arc::new(crate::handlers::MatterQcHandler::new()));
        #[cfg(feature = "produce")]
        h.register_node(Arc::new(crate::handlers::MatterProduceHandler::new()));
        #[cfg(feature = "gap")]
        h.register_node(Arc::new(crate::handlers::MatterGapHandler::new()));
        // Nested profile_run (always available; stages feature-gated inside).
        h.register_node(Arc::new(MatterProfileRunHandler::with_default_handlers()));
        h
    }

    /// Register (or replace) a node handler by its [`JobHandler::kind`].
    pub fn register_node(&mut self, handler: Arc<dyn JobHandler>) {
        self.node_handlers
            .insert(handler.kind().to_string(), handler);
    }
}

#[derive(Debug, Deserialize)]
struct WorkflowRunParams {
    #[serde(default)]
    workflow_id: Option<String>,
    #[serde(default)]
    workflow_name: Option<String>,
    #[serde(default = "empty_object")]
    run_params: Value,
}

fn empty_object() -> Value {
    Value::Object(serde_json::Map::new())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct NodeCursorEntry {
    node_id: String,
    job_id: String,
    status: String,
    kind: String,
    node_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WorkflowRunCursor {
    /// Original start params (resume identity / runner param restore).
    params: Value,
    workflow_id: String,
    workflow_name: String,
    definition_version: u32,
    /// Full body snapshot frozen at first start (not re-loaded live on resume).
    definition_body: Value,
    run_params: Value,
    nodes: Vec<NodeCursorEntry>,
}

impl JobHandler for MatterWorkflowRunHandler {
    fn kind(&self) -> &'static str {
        JOB_KIND_WORKFLOW_RUN
    }

    fn run(&self, ctx: &JobContext<'_>) -> Result<JobOutcome, RunnerError> {
        let params: WorkflowRunParams = serde_json::from_str(ctx.params_json)
            .map_err(|e| RunnerError::InvalidParams(e.to_string()))?;

        if !params.run_params.is_object() && !params.run_params.is_null() {
            return Err(RunnerError::InvalidParams(
                "workflow_run run_params must be a JSON object".into(),
            ));
        }
        let run_params = if params.run_params.is_null() {
            empty_object()
        } else {
            params.run_params.clone()
        };

        let mut cursor = load_or_init_cursor(ctx, &params, ctx.params_json, &run_params)?;

        let body = body_from_snapshot(&cursor.definition_body)?;
        // Keep the full bound plan (including disabled nodes) for audit / cursor.
        let plan = bind_workflow(&body, &cursor.run_params)
            .map_err(|e| RunnerError::InvalidParams(e.to_string()))?;
        let full_plan = plan.nodes;
        let enabled_nodes: Vec<_> = full_plan.iter().filter(|n| n.enabled).cloned().collect();
        if enabled_nodes.is_empty() {
            return Err(RunnerError::InvalidParams(format!(
                "workflow '{}' has no enabled nodes",
                cursor.workflow_name
            )));
        }

        // Audit labels cover every node; disabled ones are marked explicitly.
        let planned_labels: Vec<String> = full_plan
            .iter()
            .map(|n| {
                if n.enabled {
                    format!("{}:{}", n.node_type.as_str(), n.kind_or_profile)
                } else {
                    format!(
                        "disabled:{}:{}:{}",
                        n.node_id,
                        n.node_type.as_str(),
                        n.kind_or_profile
                    )
                }
            })
            .collect();

        // Record disabled nodes in the cursor so complete audit node_outcomes includes them.
        for n in &full_plan {
            if !n.enabled && !cursor.nodes.iter().any(|e| e.node_id == n.node_id) {
                cursor.nodes.push(NodeCursorEntry {
                    node_id: n.node_id.clone(),
                    job_id: String::new(),
                    status: "disabled".into(),
                    kind: n.kind_or_profile.clone(),
                    node_type: n.node_type.as_str().to_string(),
                });
            }
        }

        // Emit start only on a fresh run (not resume). Audit uses scrubbed run_params.
        if !ctx.is_resume {
            audit_workflow_run(
                ctx,
                "workflow_run.start",
                &cursor,
                &planned_labels,
                None,
                None,
            )?;
        }

        // Durable definition snapshot before any node runs (crash-safe for resume).
        // Checkpoint keeps full run_params (paths needed for bind/resume).
        persist_cursor(ctx, &cursor)?;

        let mut completed_nodes: u64 = cursor
            .nodes
            .iter()
            .filter(|e| e.status == "succeeded")
            .count() as u64;

        let enabled_count = enabled_nodes.len();
        ctx.progress.patch(|s| {
            s.stage = Some("workflow_run".into());
            s.message = Some(format!(
                "workflow {} ({} nodes)",
                cursor.workflow_name, enabled_count
            ));
            s.total_hint = Some(enabled_count as u64);
            s.completed_count = completed_nodes;
        });

        for (idx, node) in enabled_nodes.iter().enumerate() {
            // Skip terminal nodes from a prior attempt (resume).
            // soft_failed is terminal for that node (soft_fail already accepted).
            if cursor
                .nodes
                .iter()
                .any(|e| e.node_id == node.node_id && node_status_is_terminal_for_resume(&e.status))
            {
                continue;
            }

            if ctx.cancel.is_cancelled() {
                cascade_cancel(ctx, ctx.job_id);
                persist_cursor(ctx, &cursor)?;
                audit_workflow_run(
                    ctx,
                    "workflow_run.paused",
                    &cursor,
                    &planned_labels,
                    Some(&cursor.nodes),
                    Some("cancelled"),
                )?;
                set_parent_if_running(ctx, JobState::Paused, Some("cancelled"))?;
                return Ok(JobOutcome::Paused {
                    message: Some("cancelled".into()),
                    completed_count: completed_nodes,
                });
            }

            let node_type_str = node.node_type.as_str();
            let kind_label = node.kind_or_profile.as_str();

            ctx.progress.patch(|s| {
                s.stage = Some(format!("workflow:{kind_label}"));
                s.message = Some(format!(
                    "node {}/{}: {} ({})",
                    idx + 1,
                    enabled_count,
                    node.node_id,
                    kind_label
                ));
                s.completed_count = completed_nodes;
                s.total_hint = Some(enabled_count as u64);
            });

            let terminal = match node.node_type {
                WorkflowNodeType::Gate => {
                    run_gate_node(ctx, &mut cursor, node, kind_label, node_type_str)?
                }
                WorkflowNodeType::Job | WorkflowNodeType::ProfileRun => {
                    run_dispatch_node(self, ctx, &mut cursor, node, kind_label, node_type_str)?
                }
            };

            match terminal {
                ChildTerminal::Succeeded { .. } => {
                    completed_nodes += 1;
                    ctx.progress.patch(|s| {
                        s.completed_count = completed_nodes;
                    });
                }
                ChildTerminal::Paused {
                    message,
                    completed_count,
                } => {
                    cascade_cancel(ctx, ctx.job_id);
                    audit_workflow_run(
                        ctx,
                        "workflow_run.paused",
                        &cursor,
                        &planned_labels,
                        Some(&cursor.nodes),
                        message.as_deref(),
                    )?;
                    set_parent_if_running(ctx, JobState::Paused, message.as_deref())?;
                    return Ok(JobOutcome::Paused {
                        message,
                        completed_count: completed_nodes.max(completed_count),
                    });
                }
                ChildTerminal::Failed { message } => {
                    // Gates always hard-fail; soft_fail only for ordinary job/profile_run.
                    let allow_soft = node.soft_fail && node.node_type != WorkflowNodeType::Gate;
                    if allow_soft {
                        // Mark terminal soft_failed so resume will not re-run this node.
                        if let Some(entry) =
                            cursor.nodes.iter_mut().find(|e| e.node_id == node.node_id)
                        {
                            entry.status = "soft_failed".into();
                        }
                        persist_cursor(ctx, &cursor)?;
                        ctx.progress.patch(|s| {
                            s.message = Some(format!(
                                "node {} failed (soft_fail, continuing): {message}",
                                node.node_id
                            ));
                        });
                        continue;
                    }
                    audit_workflow_run(
                        ctx,
                        "workflow_run.failed",
                        &cursor,
                        &planned_labels,
                        Some(&cursor.nodes),
                        Some(&message),
                    )?;
                    set_parent_if_running(ctx, JobState::Failed, Some(&message))?;
                    return Ok(JobOutcome::Failed {
                        message: format!("node {} ({kind_label}) failed: {message}", node.node_id),
                    });
                }
            }
        }

        let msg = format!(
            "workflow {} complete: {} nodes",
            cursor.workflow_name, completed_nodes
        );
        audit_workflow_run(
            ctx,
            "workflow_run.complete",
            &cursor,
            &planned_labels,
            Some(&cursor.nodes),
            Some(&msg),
        )?;
        set_parent_if_running(ctx, JobState::Succeeded, None)?;

        Ok(JobOutcome::Succeeded {
            message: Some(msg),
            completed_count: completed_nodes,
        })
    }
}

/// Node statuses that must not re-execute on workflow resume.
fn node_status_is_terminal_for_resume(status: &str) -> bool {
    matches!(status, "succeeded" | "soft_failed")
}

fn run_gate_node(
    ctx: &JobContext<'_>,
    cursor: &mut WorkflowRunCursor,
    node: &matter_core::BoundNode,
    kind_label: &str,
    node_type_str: &str,
) -> Result<ChildTerminal, RunnerError> {
    let (child_id, is_resume) = resolve_or_create_child(ctx, cursor, node, kind_label)?;

    cursor.nodes.retain(|e| e.node_id != node.node_id);
    cursor.nodes.push(NodeCursorEntry {
        node_id: node.node_id.clone(),
        job_id: child_id.clone(),
        status: "running".into(),
        kind: kind_label.to_string(),
        node_type: node_type_str.to_string(),
    });
    persist_cursor(ctx, cursor)?;

    let _ = is_resume;
    let gate_result = ctx.matter.evaluate_gate(kind_label, &node.params);
    let terminal = match gate_result {
        Ok(()) => {
            let _ = ctx
                .matter
                .set_job_state(&child_id, JobState::Succeeded, None);
            ChildTerminal::Succeeded { completed_count: 1 }
        }
        Err(e) => {
            let message = e.to_string();
            let _ = ctx
                .matter
                .set_job_state(&child_id, JobState::Failed, Some(&message));
            ChildTerminal::Failed { message }
        }
    };

    if let Some(entry) = cursor.nodes.iter_mut().find(|e| e.job_id == child_id) {
        entry.status = terminal.status_str().to_string();
    }
    persist_cursor(ctx, cursor)?;
    Ok(terminal)
}

fn run_dispatch_node(
    handler_map: &MatterWorkflowRunHandler,
    ctx: &JobContext<'_>,
    cursor: &mut WorkflowRunCursor,
    node: &matter_core::BoundNode,
    kind_label: &str,
    node_type_str: &str,
) -> Result<ChildTerminal, RunnerError> {
    let handler_kind = match node.node_type {
        WorkflowNodeType::ProfileRun => JOB_KIND_PROFILE_RUN,
        WorkflowNodeType::Job => kind_label,
        WorkflowNodeType::Gate => unreachable!("gate dispatched separately"),
    };

    let handler = handler_map.node_handlers.get(handler_kind).ok_or_else(|| {
        RunnerError::HandlerFailed(format!(
            "no workflow node handler registered for kind '{handler_kind}'"
        ))
    })?;

    let params_json = build_node_params_json(node)?;
    let (child_id, is_child_resume) = resolve_or_create_child(ctx, cursor, node, handler_kind)?;

    cursor.nodes.retain(|e| e.node_id != node.node_id);
    cursor.nodes.push(NodeCursorEntry {
        node_id: node.node_id.clone(),
        job_id: child_id.clone(),
        status: "running".into(),
        kind: kind_label.to_string(),
        node_type: node_type_str.to_string(),
    });
    persist_cursor(ctx, cursor)?;

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

    if let Some(entry) = cursor.nodes.iter_mut().find(|e| e.job_id == child_id) {
        entry.status = terminal.status_str().to_string();
    }
    persist_cursor(ctx, cursor)?;
    Ok(terminal)
}

fn build_node_params_json(node: &matter_core::BoundNode) -> Result<String, RunnerError> {
    match node.node_type {
        WorkflowNodeType::ProfileRun => {
            // Merge profile identity + node params for MatterProfileRunHandler.
            let mut obj = match &node.params {
                Value::Object(m) => m.clone(),
                _ => serde_json::Map::new(),
            };
            // Prefer profile_id (builtin:/pfl_… or bare name all resolve).
            if !obj.contains_key("profile_id") && !obj.contains_key("profile_name") {
                obj.insert(
                    "profile_id".into(),
                    Value::String(node.kind_or_profile.clone()),
                );
            }
            serde_json::to_string(&Value::Object(obj))
                .map_err(|e| RunnerError::Other(format!("serialize profile_run params: {e}")))
        }
        WorkflowNodeType::Job => serde_json::to_string(&node.params)
            .map_err(|e| RunnerError::Other(format!("serialize job params: {e}"))),
        WorkflowNodeType::Gate => Ok("{}".into()),
    }
}

fn resolve_or_create_child(
    ctx: &JobContext<'_>,
    cursor: &WorkflowRunCursor,
    node: &matter_core::BoundNode,
    create_kind: &str,
) -> Result<(String, bool), RunnerError> {
    let prior = cursor
        .nodes
        .iter()
        .find(|e| e.node_id == node.node_id && e.status == "paused")
        .cloned();

    if let Some(prev) = prior {
        let job = ctx
            .matter
            .get_job(&prev.job_id)
            .map_err(RunnerError::from)?;
        if job.state == JobState::Paused {
            ctx.matter
                .set_job_state(&prev.job_id, JobState::Running, None)
                .map_err(RunnerError::from)?;
            return Ok((prev.job_id, true));
        } else if job.state == JobState::Running {
            // Orphan Running after crash — continue as resume.
            return Ok((prev.job_id, true));
        } else if job.state == JobState::Failed {
            // Hard-failed child under a paused prior entry: start a new child.
            // soft_failed nodes are skipped at the plan loop and never reach here.
            let child = ctx
                .matter
                .create_job_with_parent(create_kind, Some(ctx.job_id))
                .map_err(RunnerError::from)?;
            ctx.matter
                .set_job_state(&child.id, JobState::Running, None)
                .map_err(RunnerError::from)?;
            return Ok((child.id, false));
        }
        let child = ctx
            .matter
            .create_job_with_parent(create_kind, Some(ctx.job_id))
            .map_err(RunnerError::from)?;
        ctx.matter
            .set_job_state(&child.id, JobState::Running, None)
            .map_err(RunnerError::from)?;
        return Ok((child.id, false));
    }

    let child = ctx
        .matter
        .create_job_with_parent(create_kind, Some(ctx.job_id))
        .map_err(RunnerError::from)?;
    ctx.matter
        .set_job_state(&child.id, JobState::Running, None)
        .map_err(RunnerError::from)?;
    Ok((child.id, false))
}

/// Mark active descendants Paused (cooperative cancel), recursing into grandchildren
/// (e.g. profile_run stages under a nested profile_run node).
fn cascade_cancel(ctx: &JobContext<'_>, parent_id: &str) {
    let children = match ctx.matter.list_child_jobs(parent_id) {
        Ok(c) => c,
        Err(_) => return,
    };
    for child in children {
        match child.state {
            // Running → Paused matches profile_run cancel semantics.
            JobState::Running => {
                let _ = ctx
                    .matter
                    .set_job_state(&child.id, JobState::Paused, Some("cancelled"));
            }
            // Pending cannot transition to Paused; use Cancelled.
            JobState::Pending => {
                let _ = ctx
                    .matter
                    .set_job_state(&child.id, JobState::Cancelled, Some("cancelled"));
            }
            JobState::Paused | JobState::Succeeded | JobState::Failed | JobState::Cancelled => {}
        }
        cascade_cancel(ctx, &child.id);
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

fn body_from_snapshot(definition_body: &Value) -> Result<WorkflowBody, RunnerError> {
    let json = serde_json::to_string(definition_body)
        .map_err(|e| RunnerError::Other(format!("serialize workflow snapshot: {e}")))?;
    parse_workflow_body(&json)
        .map_err(|e| RunnerError::Other(format!("corrupt workflow definition_body snapshot: {e}")))
}

fn load_or_init_cursor(
    ctx: &JobContext<'_>,
    params: &WorkflowRunParams,
    raw_params: &str,
    run_params: &Value,
) -> Result<WorkflowRunCursor, RunnerError> {
    match ctx.matter.get_checkpoint(ctx.job_id, "workflow_run") {
        Err(e) => Err(RunnerError::from(e)),
        Ok(Some(cp)) => {
            let cursor: WorkflowRunCursor = serde_json::from_str(&cp.cursor_json)
                .map_err(|e| RunnerError::Other(format!("corrupt workflow_run checkpoint: {e}")))?;
            // Optional identity check against start params when present.
            if let Some(ref id) = params.workflow_id {
                let id = id.trim();
                if !id.is_empty() && id != cursor.workflow_id {
                    // Allow name alias: checkpoint stores resolved id.
                    if params.workflow_name.as_deref().map(str::trim)
                        != Some(cursor.workflow_name.as_str())
                        && id != cursor.workflow_name
                    {
                        return Err(RunnerError::InvalidParams(format!(
                            "workflow_run resume workflow_id mismatch: checkpoint '{}' vs params '{id}'",
                            cursor.workflow_id
                        )));
                    }
                }
            }
            Ok(cursor)
        }
        Ok(None) => {
            let resolve_key = params
                .workflow_id
                .as_deref()
                .or(params.workflow_name.as_deref())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    RunnerError::InvalidParams(
                        "workflow_run requires workflow_id or workflow_name".into(),
                    )
                })?;

            let workflow = ctx
                .matter
                .get_workflow(resolve_key)
                .map_err(|e| RunnerError::InvalidParams(e.to_string()))?;

            let definition_body = serde_json::to_value(&workflow.body).map_err(|e| {
                RunnerError::Other(format!("serialize workflow body snapshot: {e}"))
            })?;

            let params_val: Value = serde_json::from_str(raw_params).unwrap_or_else(|_| {
                json!({
                    "workflow_id": workflow.id,
                    "workflow_name": workflow.name,
                    "run_params": run_params,
                })
            });

            Ok(WorkflowRunCursor {
                params: params_val,
                workflow_id: workflow.id,
                workflow_name: workflow.name,
                definition_version: workflow.body.version,
                definition_body,
                run_params: run_params.clone(),
                nodes: Vec::new(),
            })
        }
    }
}

fn persist_cursor(ctx: &JobContext<'_>, cursor: &WorkflowRunCursor) -> Result<(), RunnerError> {
    let json = serde_json::to_string(cursor)
        .map_err(|e| RunnerError::Other(format!("serialize workflow_run cursor: {e}")))?;
    let completed = cursor
        .nodes
        .iter()
        .filter(|e| e.status == "succeeded")
        .count() as i64;
    ctx.matter
        .put_checkpoint(ctx.job_id, "workflow_run", &json, completed)
        .map_err(RunnerError::from)?;
    Ok(())
}

/// Redact secret-like keys from a JSON value for audit emission.
///
/// Walks objects recursively. Keys containing (case-insensitive) `password`,
/// `secret`, `token`, `api_key`, or `authorization` have their values replaced
/// with `"[redacted]"`. Paths and normal identity keys are preserved.
pub(crate) fn scrub_run_params_for_audit(v: &Value) -> Value {
    match v {
        Value::Object(map) => {
            let mut out = Map::new();
            for (k, val) in map {
                if key_looks_secret(k) {
                    out.insert(k.clone(), Value::String("[redacted]".into()));
                } else {
                    out.insert(k.clone(), scrub_run_params_for_audit(val));
                }
            }
            Value::Object(out)
        }
        Value::Array(arr) => Value::Array(arr.iter().map(scrub_run_params_for_audit).collect()),
        other => other.clone(),
    }
}

fn key_looks_secret(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    lower.contains("password")
        || lower.contains("secret")
        || lower.contains("token")
        || lower.contains("api_key")
        || lower.contains("authorization")
}

fn audit_workflow_run(
    ctx: &JobContext<'_>,
    action: &str,
    cursor: &WorkflowRunCursor,
    planned: &[String],
    node_outcomes: Option<&[NodeCursorEntry]>,
    message: Option<&str>,
) -> Result<(), RunnerError> {
    let definition_hash = match body_from_snapshot(&cursor.definition_body) {
        Ok(body) => workflow_definition_hash(&body),
        Err(_) => String::new(),
    };
    let mut params = json!({
        "workflow_id": cursor.workflow_id,
        "workflow_name": cursor.workflow_name,
        "definition_version": cursor.definition_version,
        "definition_hash": definition_hash,
        "nodes": planned,
        "message": message,
        "parent_job_id": ctx.job_id,
        // Scrub secrets; keep paths / source ids for provenance.
        "run_params": scrub_run_params_for_audit(&cursor.run_params),
    });
    if let Some(outcomes) = node_outcomes {
        let outcomes_val = serde_json::to_value(outcomes).map_err(|e| {
            RunnerError::Other(format!("serialize workflow_run node_outcomes: {e}"))
        })?;
        if let Some(obj) = params.as_object_mut() {
            obj.insert("node_outcomes".into(), outcomes_val);
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

#[cfg(test)]
mod scrub_tests {
    use super::scrub_run_params_for_audit;
    use serde_json::json;

    #[test]
    fn scrub_redacts_secret_keys_keeps_paths() {
        let v = json!({
            "source_path": "C:\\x",
            "token": "sekrit",
            "api_key": "k",
            "nested": { "Password": "p", "pst_item_id": "itm_1" }
        });
        let scrubbed = scrub_run_params_for_audit(&v);
        assert_eq!(scrubbed["source_path"], "C:\\x");
        assert_eq!(scrubbed["token"], "[redacted]");
        assert_eq!(scrubbed["api_key"], "[redacted]");
        assert_eq!(scrubbed["nested"]["Password"], "[redacted]");
        assert_eq!(scrubbed["nested"]["pst_item_id"], "itm_1");
    }
}
