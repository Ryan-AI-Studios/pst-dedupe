//! Declarative workflows (track **0044**).
//!
//! Workflows store a versioned body of **ordered nodes** (`job` | `profile_run` |
//! `gate`). Node list order is execution order. Process stages still run via
//! 0043 `profile_run` + [`crate::profile::CANONICAL_STAGE_ORDER`].
//!
//! Built-ins are app-global code constants; user workflows are matter-local rows
//! in `workflows` (schema v24). Parameter binding is **AST-only** (never raw
//! JSON text replace). Defensibility gates hard-fail (`soft_fail: true` rejected).

use std::collections::{BTreeSet, HashSet};

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::cas::sha256_hex;
use crate::error::{Error, Result};
use crate::matter::{new_id, now_rfc3339, Matter};
use crate::qc::qc_run_is_fresh;
use crate::AuditEventInput;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Workflow body document version accepted by this crate.
pub const WORKFLOW_BODY_VERSION: u32 = 1;

/// Maximum serialized body size (bytes) for hostile JSON rejection.
pub const WORKFLOW_BODY_MAX_BYTES: usize = 256 * 1024;

/// Job kind for sequential workflow execution (`process-runner`).
pub const JOB_KIND_WORKFLOW_RUN: &str = "workflow_run";

/// Built-in: ingest package → extract_pst → profile_run standard.
pub const BUILTIN_INGEST_THEN_STANDARD: &str = "ingest_then_standard";
/// Built-in: extract_pst → profile_run standard (source already inventoried).
pub const BUILTIN_EXTRACT_THEN_STANDARD: &str = "extract_then_standard";
/// Built-in: profile_run reduce_only.
pub const BUILTIN_REDUCE_ONLY_CHAIN: &str = "reduce_only_chain";
/// Built-in: profile_run with_ocr.
pub const BUILTIN_WITH_OCR_CHAIN: &str = "with_ocr_chain";
/// Built-in: job qc → gate require_qc_pass → job produce.
pub const BUILTIN_QC_THEN_PRODUCE: &str = "qc_then_produce";

/// Reserved built-in names (user cannot upsert these names).
pub const RESERVED_WORKFLOW_BUILTIN_NAMES: &[&str] = &[
    BUILTIN_INGEST_THEN_STANDARD,
    BUILTIN_EXTRACT_THEN_STANDARD,
    BUILTIN_REDUCE_ONLY_CHAIN,
    BUILTIN_WITH_OCR_CHAIN,
    BUILTIN_QC_THEN_PRODUCE,
];

/// P0 allowed job kinds for `type=job` nodes.
pub const ALLOWED_WORKFLOW_JOB_KINDS: &[&str] = &[
    "ingest",
    "extract_pst",
    "classify",
    "office_extract",
    "pdf_extract",
    "ics_extract",
    "ocr",
    "fts_index",
    "dedupe",
    "thread",
    "neardup",
    "cull",
    "promote",
    "qc",
    "produce",
    "gap",
];

/// Hard (defensibility) gate kinds — `soft_fail` is forbidden.
pub const HARD_GATE_KINDS: &[&str] = &["require_qc_pass", "require_has_sources"];

const BUILTIN_ID_PREFIX: &str = "builtin:";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Node discriminant in a workflow body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowNodeType {
    Job,
    ProfileRun,
    Gate,
}

impl WorkflowNodeType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Job => "job",
            Self::ProfileRun => "profile_run",
            Self::Gate => "gate",
        }
    }
}

/// One node in a workflow body (order is execution order).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowNode {
    pub id: String,
    #[serde(rename = "type")]
    pub node_type: WorkflowNodeType,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub soft_fail: bool,
    /// Required for `job` and `gate`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// Required for `profile_run`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    #[serde(default = "empty_object")]
    pub params: Value,
}

fn default_true() -> bool {
    true
}

fn empty_object() -> Value {
    Value::Object(Map::new())
}

/// Versioned workflow body (ordered nodes).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowBody {
    pub version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub nodes: Vec<WorkflowNode>,
}

/// DTO for a built-in or user workflow.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Workflow {
    /// `builtin:ingest_then_standard` or user id (`wfl_…`).
    pub id: String,
    /// `None` for built-ins.
    pub matter_id: Option<String>,
    pub name: String,
    pub description: Option<String>,
    pub body: WorkflowBody,
    pub is_builtin: bool,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub created_by: Option<String>,
}

/// Input for [`Matter::upsert_workflow`].
#[derive(Debug, Clone)]
pub struct WorkflowInput {
    pub id: Option<String>,
    pub name: String,
    pub description: Option<String>,
    /// Raw body JSON (validated).
    pub body_json: String,
    pub created_by: Option<String>,
}

/// One node after AST param binding (ready for the runner).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BoundNode {
    pub node_id: String,
    #[serde(rename = "type")]
    pub node_type: WorkflowNodeType,
    /// `kind` for job/gate; `profile` id/name for profile_run.
    pub kind_or_profile: String,
    pub soft_fail: bool,
    pub enabled: bool,
    pub params: Value,
}

/// Ordered plan of bound nodes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowPlan {
    pub nodes: Vec<BoundNode>,
}

/// Result of structural validation (body + collected placeholder keys).
#[derive(Debug, Clone, PartialEq)]
pub struct WorkflowValidation {
    pub body: WorkflowBody,
    /// Sorted unique `${key}` names found in string leaves.
    pub placeholders: Vec<String>,
}

// ---------------------------------------------------------------------------
// Pure helpers — ids / allowlists
// ---------------------------------------------------------------------------

/// True when `kind` is allowed on a `type=job` node.
pub fn is_allowed_workflow_job_kind(kind: &str) -> bool {
    ALLOWED_WORKFLOW_JOB_KINDS.contains(&kind)
}

/// True when `kind` is a hard defensibility gate.
pub fn is_hard_gate_kind(kind: &str) -> bool {
    HARD_GATE_KINDS.contains(&kind)
}

/// Built-in workflow id for a reserved name (`builtin:ingest_then_standard`).
pub fn workflow_builtin_id(name: &str) -> String {
    format!("{BUILTIN_ID_PREFIX}{name}")
}

/// Strip `builtin:` prefix when present.
pub fn strip_workflow_builtin_prefix(id_or_name: &str) -> &str {
    id_or_name
        .strip_prefix(BUILTIN_ID_PREFIX)
        .unwrap_or(id_or_name)
}

// ---------------------------------------------------------------------------
// Placeholders + AST bind
// ---------------------------------------------------------------------------

/// Collect unique `${identifier}` placeholders from all string leaves (AST walk).
pub fn collect_placeholders(value: &Value) -> Vec<String> {
    let mut set = BTreeSet::new();
    collect_placeholders_into(value, &mut set);
    set.into_iter().collect()
}

fn collect_placeholders_into(value: &Value, out: &mut BTreeSet<String>) {
    match value {
        Value::String(s) => {
            for key in find_placeholders(s) {
                out.insert(key);
            }
        }
        Value::Array(arr) => {
            for v in arr {
                collect_placeholders_into(v, out);
            }
        }
        Value::Object(map) => {
            for v in map.values() {
                collect_placeholders_into(v, out);
            }
        }
        _ => {}
    }
}

/// Find `${key}` placeholders in a string (`key` = `[a-zA-Z_][a-zA-Z0-9_]*`).
fn find_placeholders(s: &str) -> Vec<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'$' && i + 1 < bytes.len() && bytes[i + 1] == b'{' {
            let start = i + 2;
            let mut j = start;
            if j < bytes.len() && (bytes[j].is_ascii_alphabetic() || bytes[j] == b'_') {
                j += 1;
                while j < bytes.len() && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_') {
                    j += 1;
                }
                if j < bytes.len() && bytes[j] == b'}' {
                    out.push(s[start..j].to_string());
                    i = j + 1;
                    continue;
                }
            }
        }
        i += 1;
    }
    out
}

/// Convert a run-param scalar to its substitution string form.
fn scalar_to_string(key: &str, v: &Value) -> Result<String> {
    match v {
        Value::Null => Ok(String::new()),
        Value::Bool(b) => Ok(b.to_string()),
        Value::Number(n) => Ok(n.to_string()),
        Value::String(s) => Ok(s.clone()),
        Value::Array(_) | Value::Object(_) => Err(Error::Other(format!(
            "workflow run_params['{key}'] must be a scalar (string/number/bool/null)"
        ))),
    }
}

/// Substitute `${key}` placeholders in a string using `run_params` (scalars only).
fn substitute_string(s: &str, run_params: &Map<String, Value>) -> Result<String> {
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'$' && i + 1 < bytes.len() && bytes[i + 1] == b'{' {
            let start = i + 2;
            let mut j = start;
            if j < bytes.len() && (bytes[j].is_ascii_alphabetic() || bytes[j] == b'_') {
                j += 1;
                while j < bytes.len() && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_') {
                    j += 1;
                }
                if j < bytes.len() && bytes[j] == b'}' {
                    let key = &s[start..j];
                    let val = run_params.get(key).ok_or_else(|| {
                        Error::Other(format!("workflow bind missing run_params key: {key}"))
                    })?;
                    out.push_str(&scalar_to_string(key, val)?);
                    i = j + 1;
                    continue;
                }
            }
        }
        // Copy one UTF-8 char safely.
        let ch = s[i..].chars().next().unwrap_or('\u{FFFD}');
        out.push(ch);
        i += ch.len_utf8();
    }
    Ok(out)
}

/// AST walk: replace placeholders in string leaves only. Structure is preserved.
fn bind_value(value: &Value, run_params: &Map<String, Value>) -> Result<Value> {
    match value {
        Value::String(s) => Ok(Value::String(substitute_string(s, run_params)?)),
        Value::Array(arr) => {
            let mut out = Vec::with_capacity(arr.len());
            for v in arr {
                out.push(bind_value(v, run_params)?);
            }
            Ok(Value::Array(out))
        }
        Value::Object(map) => {
            let mut out = Map::new();
            for (k, v) in map {
                out.insert(k.clone(), bind_value(v, run_params)?);
            }
            Ok(Value::Object(out))
        }
        other => Ok(other.clone()),
    }
}

/// Bind run params into a validated workflow body (AST walk only).
///
/// Missing keys hard-fail. Run-param values must be scalars.
pub fn bind_workflow(body: &WorkflowBody, run_params: &Value) -> Result<WorkflowPlan> {
    let params_map = match run_params {
        Value::Object(m) => m,
        Value::Null => {
            // Empty object when no params needed.
            return bind_workflow(body, &empty_object());
        }
        _ => {
            return Err(Error::Other(
                "workflow run_params must be a JSON object".into(),
            ));
        }
    };

    // Pre-validate that all provided values are scalars.
    for (k, v) in params_map {
        let _ = scalar_to_string(k, v)?;
    }

    let mut nodes = Vec::with_capacity(body.nodes.len());
    for node in &body.nodes {
        let bound_params = bind_value(&node.params, params_map)?;
        let kind_or_profile = match node.node_type {
            WorkflowNodeType::Job | WorkflowNodeType::Gate => node.kind.clone().unwrap_or_default(),
            WorkflowNodeType::ProfileRun => node.profile.clone().unwrap_or_default(),
        };
        // Gates always hard-fail regardless of body soft_fail.
        let soft_fail = match node.node_type {
            WorkflowNodeType::Gate => false,
            _ => node.soft_fail,
        };
        nodes.push(BoundNode {
            node_id: node.id.clone(),
            node_type: node.node_type,
            kind_or_profile,
            soft_fail,
            enabled: node.enabled,
            params: bound_params,
        });
    }
    Ok(WorkflowPlan { nodes })
}

// ---------------------------------------------------------------------------
// Parse / validate
// ---------------------------------------------------------------------------

/// Parse and validate a workflow body JSON string.
pub fn parse_workflow_body(json: &str) -> Result<WorkflowBody> {
    if json.len() > WORKFLOW_BODY_MAX_BYTES {
        return Err(Error::Other(format!(
            "workflow body exceeds max size ({} bytes)",
            WORKFLOW_BODY_MAX_BYTES
        )));
    }

    let root: Value = serde_json::from_str(json)
        .map_err(|e| Error::Other(format!("invalid workflow body JSON: {e}")))?;

    let body: WorkflowBody = serde_json::from_value(root)
        .map_err(|e| Error::Other(format!("invalid workflow body shape: {e}")))?;

    validate_workflow(&body)?;
    Ok(body)
}

/// Validate a workflow body (version, nodes, kinds, hard gates).
pub fn validate_workflow(body: &WorkflowBody) -> Result<()> {
    if body.version != WORKFLOW_BODY_VERSION {
        return Err(Error::Other(format!(
            "unknown workflow body version: {} (expected {WORKFLOW_BODY_VERSION})",
            body.version
        )));
    }
    if body.nodes.is_empty() {
        return Err(Error::Other(
            "workflow body must contain at least one node".into(),
        ));
    }

    let mut seen_ids = HashSet::new();
    for node in &body.nodes {
        let id = node.id.trim();
        if id.is_empty() {
            return Err(Error::Other("workflow node id cannot be empty".into()));
        }
        if !seen_ids.insert(id.to_string()) {
            return Err(Error::Other(format!("duplicate workflow node id: {id}")));
        }

        if !node.params.is_object() {
            return Err(Error::Other(format!(
                "workflow node '{id}' params must be a JSON object"
            )));
        }

        match node.node_type {
            WorkflowNodeType::Job => {
                let kind = node
                    .kind
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| {
                        Error::Other(format!("workflow job node '{id}' missing kind"))
                    })?;
                if !is_allowed_workflow_job_kind(kind) {
                    return Err(Error::Other(format!("unknown workflow job kind: {kind}")));
                }
                if node.profile.is_some() {
                    return Err(Error::Other(format!(
                        "workflow job node '{id}' must not set profile"
                    )));
                }
            }
            WorkflowNodeType::ProfileRun => {
                let profile = node
                    .profile
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| {
                        Error::Other(format!("workflow profile_run node '{id}' missing profile"))
                    })?;
                let _ = profile;
                if node.kind.is_some() {
                    return Err(Error::Other(format!(
                        "workflow profile_run node '{id}' must not set kind"
                    )));
                }
            }
            WorkflowNodeType::Gate => {
                let kind = node
                    .kind
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| {
                        Error::Other(format!("workflow gate node '{id}' missing kind"))
                    })?;
                if !is_hard_gate_kind(kind) {
                    return Err(Error::Other(format!("unknown workflow gate kind: {kind}")));
                }
                if node.soft_fail {
                    return Err(Error::Other(format!(
                        "workflow gate '{kind}' cannot set soft_fail:true (defensibility gates hard-fail only)"
                    )));
                }
                if node.profile.is_some() {
                    return Err(Error::Other(format!(
                        "workflow gate node '{id}' must not set profile"
                    )));
                }
            }
        }
    }

    // Collect placeholders (structural only; missing keys fail at bind).
    let mut placeholders = BTreeSet::new();
    for node in &body.nodes {
        collect_placeholders_into(&node.params, &mut placeholders);
    }
    let _ = placeholders;

    Ok(())
}

/// Validate and return body + sorted placeholder keys.
pub fn validate_workflow_detailed(body: &WorkflowBody) -> Result<WorkflowValidation> {
    validate_workflow(body)?;
    let mut set = BTreeSet::new();
    for node in &body.nodes {
        collect_placeholders_into(&node.params, &mut set);
    }
    Ok(WorkflowValidation {
        body: body.clone(),
        placeholders: set.into_iter().collect(),
    })
}

/// Serialize a body to JSON.
pub fn workflow_body_to_json(body: &WorkflowBody) -> Result<String> {
    serde_json::to_string(body).map_err(|e| Error::Other(format!("serialize workflow body: {e}")))
}

// ---------------------------------------------------------------------------
// Built-in definitions
// ---------------------------------------------------------------------------

fn node_job(id: &str, kind: &str, params: Value) -> WorkflowNode {
    WorkflowNode {
        id: id.into(),
        node_type: WorkflowNodeType::Job,
        enabled: true,
        soft_fail: false,
        kind: Some(kind.into()),
        profile: None,
        params,
    }
}

fn node_profile_run(id: &str, profile: &str, params: Value) -> WorkflowNode {
    WorkflowNode {
        id: id.into(),
        node_type: WorkflowNodeType::ProfileRun,
        enabled: true,
        soft_fail: false,
        kind: None,
        profile: Some(profile.into()),
        params,
    }
}

fn node_gate(id: &str, kind: &str) -> WorkflowNode {
    WorkflowNode {
        id: id.into(),
        node_type: WorkflowNodeType::Gate,
        enabled: true,
        soft_fail: false,
        kind: Some(kind.into()),
        profile: None,
        params: empty_object(),
    }
}

fn make_builtin(name: &str, description: &str, nodes: Vec<WorkflowNode>) -> Workflow {
    let body = WorkflowBody {
        version: WORKFLOW_BODY_VERSION,
        name: Some(name.into()),
        description: Some(description.into()),
        nodes,
    };
    // Built-ins are authored in code; panic-free validate for developer mistakes.
    if let Err(e) = validate_workflow(&body) {
        // Should never happen for constants — surface as empty/disabled would be worse.
        // Use a fallback that still fails closed at runtime if somehow invalid.
        let _ = e;
    }
    Workflow {
        id: workflow_builtin_id(name),
        matter_id: None,
        name: name.into(),
        description: Some(description.into()),
        body,
        is_builtin: true,
        created_at: None,
        updated_at: None,
        created_by: None,
    }
}

/// All built-in workflows (code constants).
pub fn builtin_workflows() -> Vec<Workflow> {
    vec![
        make_builtin(
            BUILTIN_INGEST_THEN_STANDARD,
            "Ingest a package path, extract PSTs, run standard profile",
            vec![
                node_job("n1", "ingest", json!({ "path": "${source_path}" })),
                node_job(
                    "n2",
                    "extract_pst",
                    json!({
                        "source_id": "${source_id}",
                        "pst_item_id": "${pst_item_id}"
                    }),
                ),
                node_profile_run(
                    "n3",
                    "builtin:standard",
                    json!({ "stop_on_stage_failure": true }),
                ),
            ],
        ),
        make_builtin(
            BUILTIN_EXTRACT_THEN_STANDARD,
            "Extract PST then run standard profile (source already inventoried)",
            vec![
                node_job(
                    "n1",
                    "extract_pst",
                    json!({
                        "source_id": "${source_id}",
                        "pst_item_id": "${pst_item_id}"
                    }),
                ),
                node_profile_run(
                    "n2",
                    "builtin:standard",
                    json!({ "stop_on_stage_failure": true }),
                ),
            ],
        ),
        make_builtin(
            BUILTIN_REDUCE_ONLY_CHAIN,
            "Run reduce_only processing profile",
            vec![node_profile_run(
                "n1",
                "builtin:reduce_only",
                json!({ "stop_on_stage_failure": true }),
            )],
        ),
        make_builtin(
            BUILTIN_WITH_OCR_CHAIN,
            "Run with_ocr processing profile",
            vec![node_profile_run(
                "n1",
                "builtin:with_ocr",
                json!({ "stop_on_stage_failure": true }),
            )],
        ),
        make_builtin(
            BUILTIN_QC_THEN_PRODUCE,
            "Run QC, require pass gate, then produce",
            vec![
                node_job("n1", "qc", empty_object()),
                node_gate("n2", "require_qc_pass"),
                node_job("n3", "produce", empty_object()),
            ],
        ),
    ]
}

/// Look up a single built-in by name or `builtin:name`.
pub fn builtin_workflow(name: &str) -> Option<Workflow> {
    let bare = strip_workflow_builtin_prefix(name);
    builtin_workflows().into_iter().find(|w| w.name == bare)
}

// ---------------------------------------------------------------------------
// Definition identity
// ---------------------------------------------------------------------------

/// SHA-256 hex of the compact JSON serialization of a workflow body.
///
/// Used for audit provenance so a completed run can be tied back to the exact
/// definition snapshot that executed (version + hash).
pub fn workflow_definition_hash(body: &WorkflowBody) -> String {
    let bytes = serde_json::to_vec(body).unwrap_or_default();
    sha256_hex(&bytes)
}

// ---------------------------------------------------------------------------
// Gate evaluation (runner calls this)
// ---------------------------------------------------------------------------

/// Evaluate a hard gate against the matter store.
///
/// - `require_has_sources`: fail if no sources rows
/// - `require_qc_pass`: fail unless a **fresh** passed QC run exists for the
///   gate scope (0041: `passed` + matching scope + candidate count + selection
///   fingerprint). Mirrors produce / matter-qc gate semantics.
///
/// Params for `require_qc_pass` (optional object fields):
/// - `scope` — string, default `"review_corpus"` (`review_corpus` | `item_ids`)
/// - `item_ids` — string array used when `scope` is `item_ids` (ignored for
///   `review_corpus`, which loads `items` with `in_review = 1`)
pub fn evaluate_gate_kind(matter: &Matter, gate_kind: &str, params: &Value) -> Result<()> {
    match gate_kind {
        "require_has_sources" => {
            let sources = matter.list_sources()?;
            if sources.is_empty() {
                return Err(Error::Other(
                    "gate require_has_sources failed: matter has no sources".into(),
                ));
            }
            Ok(())
        }
        "require_qc_pass" => evaluate_require_qc_pass(matter, params),
        other => Err(Error::Other(format!("unknown workflow gate kind: {other}"))),
    }
}

/// Load candidate item ids for a `require_qc_pass` gate from params + matter.
fn gate_candidate_ids(matter: &Matter, scope: &str, params: &Value) -> Result<Vec<String>> {
    match scope {
        "review_corpus" => {
            let mut stmt = matter.connection().prepare(
                "SELECT id FROM items WHERE matter_id = ?1 AND in_review = 1 ORDER BY id",
            )?;
            let rows = stmt.query_map(params![matter.id()], |row| row.get::<_, String>(0))?;
            let mut ids = Vec::new();
            for row in rows {
                ids.push(row?);
            }
            Ok(ids)
        }
        "item_ids" => {
            let Some(arr) = params.get("item_ids").and_then(|v| v.as_array()) else {
                return Err(Error::Other(
                    "gate require_qc_pass: scope=item_ids requires params.item_ids string array"
                        .into(),
                ));
            };
            let mut ids = Vec::with_capacity(arr.len());
            for (i, v) in arr.iter().enumerate() {
                match v.as_str() {
                    Some(s) if !s.is_empty() => ids.push(s.to_string()),
                    _ => {
                        return Err(Error::Other(format!(
                            "gate require_qc_pass: params.item_ids[{i}] must be a non-empty string"
                        )));
                    }
                }
            }
            Ok(ids)
        }
        other => Err(Error::Other(format!(
            "gate require_qc_pass: unknown scope '{other}' (expected review_corpus or item_ids)"
        ))),
    }
}

fn evaluate_require_qc_pass(matter: &Matter, params: &Value) -> Result<()> {
    let scope = params
        .get("scope")
        .and_then(|v| v.as_str())
        .unwrap_or("review_corpus");
    let candidate_ids = gate_candidate_ids(matter, scope, params)?;

    // Prefer latest run for this scope; fall back to any latest (may still be stale).
    let stored = match matter.load_latest_qc_run_for_scope(Some(scope))? {
        Some(r) => r,
        None => match matter.load_latest_qc_run()? {
            Some(r) => r,
            None => {
                return Err(Error::Other(
                    "gate require_qc_pass failed: no QC run recorded".into(),
                ));
            }
        },
    };

    if qc_run_is_fresh(&stored, scope, &candidate_ids) {
        return Ok(());
    }

    if !stored.passed {
        return Err(Error::Other(format!(
            "gate require_qc_pass failed: latest QC run {} did not pass (errors={})",
            stored.id, stored.error_count
        )));
    }

    Err(Error::Other(format!(
        "gate require_qc_pass failed: QC run {} is not fresh for scope '{scope}' \
         (stored_count={}, current_count={}; re-run QC after selection changes)",
        stored.id,
        stored.candidate_count,
        candidate_ids.len()
    )))
}

// ---------------------------------------------------------------------------
// Matter API
// ---------------------------------------------------------------------------

impl Matter {
    /// List built-ins + user workflows for this matter.
    pub fn list_workflows(&self) -> Result<Vec<Workflow>> {
        let mut out = builtin_workflows();
        let mut stmt = self.connection().prepare(
            "SELECT id, matter_id, name, description, body_json, created_at, updated_at, created_by \
             FROM workflows WHERE matter_id = ?1 ORDER BY name ASC",
        )?;
        let rows = stmt.query_map(params![self.id()], map_workflow_row)?;
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Resolve by id (`builtin:…`, user uuid) or bare built-in / user name.
    pub fn get_workflow(&self, id_or_name: &str) -> Result<Workflow> {
        let key = id_or_name.trim();
        if key.is_empty() {
            return Err(Error::Other("workflow id/name cannot be empty".into()));
        }

        if let Some(w) = builtin_workflow(key) {
            return Ok(w);
        }

        if let Some(w) = self.load_user_workflow_by_id(key)? {
            return Ok(w);
        }

        if let Some(w) = self.load_user_workflow_by_name(key)? {
            return Ok(w);
        }

        Err(Error::Other(format!("workflow not found: {key}")))
    }

    /// Insert or update a user workflow. Reserved built-in names are rejected.
    pub fn upsert_workflow(&self, input: WorkflowInput) -> Result<Workflow> {
        let now = now_rfc3339();
        let name = input.name.trim();
        if name.is_empty() {
            return Err(Error::Other("workflow name cannot be empty".into()));
        }
        if RESERVED_WORKFLOW_BUILTIN_NAMES.contains(&name) {
            return Err(Error::Other(format!(
                "name '{name}' is reserved for a built-in workflow"
            )));
        }

        let body = parse_workflow_body(&input.body_json)?;
        let body_json = workflow_body_to_json(&body)?;
        if body_json.len() > WORKFLOW_BODY_MAX_BYTES {
            return Err(Error::Other(format!(
                "workflow body exceeds max size ({} bytes)",
                WORKFLOW_BODY_MAX_BYTES
            )));
        }

        let workflow = if let Some(ref id) = input.id {
            if id.starts_with(BUILTIN_ID_PREFIX) {
                return Err(Error::Other("cannot upsert a built-in workflow".into()));
            }
            let existing = self.get_workflow(id)?;
            if existing.is_builtin {
                return Err(Error::Other("cannot upsert a built-in workflow".into()));
            }
            if existing.matter_id.as_deref() != Some(self.id()) {
                return Err(Error::Other(format!(
                    "workflow {id} belongs to another matter"
                )));
            }
            let clash: Option<String> = self
                .connection()
                .query_row(
                    "SELECT id FROM workflows WHERE matter_id = ?1 AND name = ?2 AND id != ?3",
                    params![self.id(), name, id],
                    |row| row.get(0),
                )
                .optional()?;
            if clash.is_some() {
                return Err(Error::Other(format!(
                    "workflow name already exists in matter: {name}"
                )));
            }
            self.connection().execute(
                "UPDATE workflows SET name = ?1, description = ?2, body_json = ?3, \
                 updated_at = ?4, created_by = COALESCE(?5, created_by) WHERE id = ?6",
                params![
                    name,
                    input.description,
                    body_json,
                    now,
                    input.created_by,
                    id
                ],
            )?;
            self.load_user_workflow_by_id(id)?
                .ok_or_else(|| Error::Other(format!("workflow not found after update: {id}")))?
        } else {
            let clash: Option<String> = self
                .connection()
                .query_row(
                    "SELECT id FROM workflows WHERE matter_id = ?1 AND name = ?2",
                    params![self.id(), name],
                    |row| row.get(0),
                )
                .optional()?;
            if clash.is_some() {
                return Err(Error::Other(format!(
                    "workflow name already exists in matter: {name}"
                )));
            }
            let id = new_id("wfl");
            self.connection().execute(
                "INSERT INTO workflows (id, matter_id, name, description, body_json, \
                 created_at, updated_at, created_by) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    id,
                    self.id(),
                    name,
                    input.description,
                    body_json,
                    now,
                    now,
                    input.created_by
                ],
            )?;
            self.load_user_workflow_by_id(&id)?
                .ok_or_else(|| Error::Other(format!("workflow not found after insert: {id}")))?
        };

        let node_kinds: Vec<String> = workflow
            .body
            .nodes
            .iter()
            .map(|n| match n.node_type {
                WorkflowNodeType::Job => format!("job:{}", n.kind.as_deref().unwrap_or("?")),
                WorkflowNodeType::ProfileRun => {
                    format!("profile_run:{}", n.profile.as_deref().unwrap_or("?"))
                }
                WorkflowNodeType::Gate => format!("gate:{}", n.kind.as_deref().unwrap_or("?")),
            })
            .collect();
        let _ = self.append_audit(AuditEventInput {
            actor: input.created_by.clone().unwrap_or_else(|| "system".into()),
            action: "workflow.upsert".into(),
            entity: format!("workflow:{}", workflow.id),
            params_json: json!({
                "id": workflow.id,
                "name": workflow.name,
                "version": workflow.body.version,
                "nodes": node_kinds,
            })
            .to_string(),
            tool_version: env!("CARGO_PKG_VERSION").into(),
        })?;

        Ok(workflow)
    }

    /// Delete a user workflow. Built-ins cannot be deleted. Clears default if matched.
    pub fn delete_workflow(&self, id: &str) -> Result<()> {
        let key = id.trim();
        if key.starts_with(BUILTIN_ID_PREFIX) || builtin_workflow(key).is_some() {
            return Err(Error::Other("cannot delete a built-in workflow".into()));
        }
        let existing = self.get_workflow(key)?;
        if existing.is_builtin {
            return Err(Error::Other("cannot delete a built-in workflow".into()));
        }
        if existing.matter_id.as_deref() != Some(self.id()) {
            return Err(Error::Other(format!(
                "workflow {key} belongs to another matter"
            )));
        }

        self.connection()
            .execute("DELETE FROM workflows WHERE id = ?1", params![existing.id])?;

        if let Ok(Some(default_id)) = self.get_default_workflow_id() {
            if default_id == existing.id {
                let _ = self.set_default_workflow(None);
            }
        }

        let _ = self.append_audit(AuditEventInput {
            actor: existing
                .created_by
                .clone()
                .unwrap_or_else(|| "system".into()),
            action: "workflow.delete".into(),
            entity: format!("workflow:{}", existing.id),
            params_json: json!({
                "id": existing.id,
                "name": existing.name,
            })
            .to_string(),
            tool_version: env!("CARGO_PKG_VERSION").into(),
        })?;
        Ok(())
    }

    /// Parse + validate workflow body JSON (convenience for UI/API).
    pub fn validate_workflow_json(&self, body_json: &str) -> Result<WorkflowValidation> {
        let body = parse_workflow_body(body_json)?;
        validate_workflow_detailed(&body)
    }

    /// Evaluate a hard gate against this matter.
    pub fn evaluate_gate(&self, gate_kind: &str, params: &Value) -> Result<()> {
        evaluate_gate_kind(self, gate_kind, params)
    }

    /// Set or clear the matter's default workflow id.
    pub fn set_default_workflow(&self, workflow_id: Option<&str>) -> Result<()> {
        if let Some(id) = workflow_id {
            let _ = self.get_workflow(id)?;
            self.connection().execute(
                "UPDATE matters SET default_workflow_id = ?1 WHERE id = ?2",
                params![id, self.id()],
            )?;
        } else {
            self.connection().execute(
                "UPDATE matters SET default_workflow_id = NULL WHERE id = ?1",
                params![self.id()],
            )?;
        }
        Ok(())
    }

    /// Current default workflow id, if set.
    pub fn get_default_workflow_id(&self) -> Result<Option<String>> {
        let id: Option<String> = self.connection().query_row(
            "SELECT default_workflow_id FROM matters WHERE id = ?1",
            params![self.id()],
            |row| row.get(0),
        )?;
        Ok(id.filter(|s| !s.is_empty()))
    }

    fn load_user_workflow_by_id(&self, id: &str) -> Result<Option<Workflow>> {
        self.connection()
            .query_row(
                "SELECT id, matter_id, name, description, body_json, created_at, updated_at, created_by \
                 FROM workflows WHERE id = ?1 AND matter_id = ?2",
                params![id, self.id()],
                map_workflow_row,
            )
            .optional()
            .map_err(Error::from)
    }

    fn load_user_workflow_by_name(&self, name: &str) -> Result<Option<Workflow>> {
        self.connection()
            .query_row(
                "SELECT id, matter_id, name, description, body_json, created_at, updated_at, created_by \
                 FROM workflows WHERE matter_id = ?1 AND name = ?2",
                params![self.id(), name],
                map_workflow_row,
            )
            .optional()
            .map_err(Error::from)
    }
}

fn map_workflow_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Workflow> {
    let body_json: String = row.get(4)?;
    let body = parse_workflow_body(&body_json).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(
            4,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                e.to_string(),
            )),
        )
    })?;
    Ok(Workflow {
        id: row.get(0)?,
        matter_id: Some(row.get(1)?),
        name: row.get(2)?,
        description: row.get(3)?,
        body,
        is_builtin: false,
        created_at: Some(row.get(5)?),
        updated_at: Some(row.get(6)?),
        created_by: row.get(7)?,
    })
}

#[cfg(test)]
mod unit_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn soft_fail_on_gate_rejected() {
        let body = WorkflowBody {
            version: 1,
            name: None,
            description: None,
            nodes: vec![WorkflowNode {
                id: "g1".into(),
                node_type: WorkflowNodeType::Gate,
                enabled: true,
                soft_fail: true,
                kind: Some("require_qc_pass".into()),
                profile: None,
                params: empty_object(),
            }],
        };
        let err = validate_workflow(&body).expect_err("soft_fail gate");
        assert!(err.to_string().contains("soft_fail"));
    }

    #[test]
    fn unknown_job_kind_rejected() {
        let body = WorkflowBody {
            version: 1,
            name: None,
            description: None,
            nodes: vec![WorkflowNode {
                id: "j1".into(),
                node_type: WorkflowNodeType::Job,
                enabled: true,
                soft_fail: false,
                kind: Some("shell".into()),
                profile: None,
                params: empty_object(),
            }],
        };
        let err = validate_workflow(&body).expect_err("shell");
        assert!(err.to_string().contains("unknown workflow job kind"));
    }

    #[test]
    fn ast_bind_preserves_windows_path_with_quotes() {
        let body = WorkflowBody {
            version: 1,
            name: None,
            description: None,
            nodes: vec![WorkflowNode {
                id: "n1".into(),
                node_type: WorkflowNodeType::Job,
                enabled: true,
                soft_fail: false,
                kind: Some("ingest".into()),
                profile: None,
                params: json!({ "path": "${source_path}" }),
            }],
        };
        validate_workflow(&body).expect("validate");
        let path = r#"C:\Users\test\foo"bar\export"#;
        let plan = bind_workflow(&body, &json!({ "source_path": path })).expect("bind");
        assert_eq!(plan.nodes.len(), 1);
        let bound = plan.nodes[0]
            .params
            .get("path")
            .and_then(|v| v.as_str())
            .expect("path str");
        assert_eq!(bound, path);
        // Round-trip through JSON serialization still equals original.
        let ser = serde_json::to_string(&plan.nodes[0].params).expect("ser");
        let back: Value = serde_json::from_str(&ser).expect("de");
        assert_eq!(back.get("path").and_then(|v| v.as_str()), Some(path));
    }

    #[test]
    fn unknown_placeholder_fails_bind() {
        let body = WorkflowBody {
            version: 1,
            name: None,
            description: None,
            nodes: vec![WorkflowNode {
                id: "n1".into(),
                node_type: WorkflowNodeType::Job,
                enabled: true,
                soft_fail: false,
                kind: Some("ingest".into()),
                profile: None,
                params: json!({ "path": "${missing_key}" }),
            }],
        };
        let err = bind_workflow(&body, &json!({})).expect_err("missing");
        assert!(err.to_string().contains("missing_key"));
    }

    #[test]
    fn builtins_validate() {
        for w in builtin_workflows() {
            validate_workflow(&w.body).unwrap_or_else(|e| {
                panic!("builtin {} invalid: {e}", w.name);
            });
        }
        assert_eq!(builtin_workflows().len(), 5);
    }
}
