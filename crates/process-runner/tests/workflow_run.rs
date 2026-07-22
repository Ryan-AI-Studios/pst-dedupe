//! Integration tests for `workflow_run` (track 0044).

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use camino::Utf8PathBuf;
use matter_core::{
    JobState, Matter, ProcessingProfileInput, WorkflowInput, JOB_KIND_PROFILE_RUN,
    JOB_KIND_WORKFLOW_RUN, SCHEMA_VERSION,
};
use process_runner::{
    JobContext, JobHandler, JobOutcome, JobParams, MatterProfileRunHandler,
    MatterWorkflowRunHandler, ProcessRunner, RunnerConfig, RunnerError,
};
use tempfile::tempdir;

fn utf8_tempdir() -> (tempfile::TempDir, Utf8PathBuf) {
    let dir = tempdir().expect("tempdir");
    let path = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8");
    (dir, path)
}

fn make_matter(base: &Utf8PathBuf, name: &str) -> Utf8PathBuf {
    let root = base.join(name);
    Matter::create(&root, name).expect("create matter");
    root
}

/// Fast succeed for allowlisted kind.
struct OkNodeHandler {
    kind: &'static str,
}

impl JobHandler for OkNodeHandler {
    fn kind(&self) -> &'static str {
        self.kind
    }

    fn run(&self, ctx: &JobContext<'_>) -> Result<JobOutcome, RunnerError> {
        ctx.matter
            .set_job_state(ctx.job_id, JobState::Succeeded, None)
            .map_err(RunnerError::from)?;
        Ok(JobOutcome::Succeeded {
            message: Some(format!("{} ok", self.kind)),
            completed_count: 1,
        })
    }
}

/// Always-fail for allowlisted kind.
struct FailNodeHandler {
    kind: &'static str,
}

impl JobHandler for FailNodeHandler {
    fn kind(&self) -> &'static str {
        self.kind
    }

    fn run(&self, ctx: &JobContext<'_>) -> Result<JobOutcome, RunnerError> {
        ctx.matter
            .set_job_state(ctx.job_id, JobState::Failed, Some("injected fail"))
            .map_err(RunnerError::from)?;
        Ok(JobOutcome::Failed {
            message: "injected fail".into(),
        })
    }
}

/// Slow allowlisted node that honors cancel.
struct SlowNodeHandler {
    kind: &'static str,
    ticks: Arc<AtomicUsize>,
}

impl JobHandler for SlowNodeHandler {
    fn kind(&self) -> &'static str {
        self.kind
    }

    fn run(&self, ctx: &JobContext<'_>) -> Result<JobOutcome, RunnerError> {
        for _ in 0..500 {
            if ctx.cancel.is_cancelled() {
                ctx.matter
                    .set_job_state(ctx.job_id, JobState::Paused, Some("cancelled"))
                    .map_err(RunnerError::from)?;
                return Ok(JobOutcome::Paused {
                    message: Some("cancelled".into()),
                    completed_count: self.ticks.load(Ordering::SeqCst) as u64,
                });
            }
            self.ticks.fetch_add(1, Ordering::SeqCst);
            thread::sleep(Duration::from_millis(20));
        }
        ctx.matter
            .set_job_state(ctx.job_id, JobState::Succeeded, None)
            .map_err(RunnerError::from)?;
        Ok(JobOutcome::Succeeded {
            message: Some("slow finished".into()),
            completed_count: self.ticks.load(Ordering::SeqCst) as u64,
        })
    }
}

#[test]
fn schema_version_is_current() {
    assert_eq!(SCHEMA_VERSION, 39);
    let (_tmp, base) = utf8_tempdir();
    let root = make_matter(&base, "m-schema");
    let matter = Matter::open(&root).expect("open");
    assert_eq!(matter.schema_version().expect("ver"), SCHEMA_VERSION);
}

#[test]
fn workflow_two_node_job_chain_succeeds() {
    let (_tmp, base) = utf8_tempdir();
    let root = make_matter(&base, "m-wf-ok");

    {
        let matter = Matter::open(&root).expect("open");
        let body = r#"{
            "version": 1,
            "nodes": [
                { "id": "n1", "type": "job", "kind": "dedupe", "params": {} },
                { "id": "n2", "type": "job", "kind": "thread", "params": {} }
            ]
        }"#;
        matter
            .upsert_workflow(WorkflowInput {
                id: None,
                name: "two_jobs".into(),
                description: None,
                body_json: body.into(),
                created_by: None,
            })
            .expect("upsert workflow");
    }

    let mut handler = MatterWorkflowRunHandler::new();
    handler.register_node(Arc::new(OkNodeHandler { kind: "dedupe" }));
    handler.register_node(Arc::new(OkNodeHandler { kind: "thread" }));

    let mut runner = ProcessRunner::new(RunnerConfig::default());
    runner.register(Arc::new(handler));

    let parent_id = runner
        .start(
            &root,
            JOB_KIND_WORKFLOW_RUN,
            JobParams::new(r#"{"workflow_name":"two_jobs"}"#),
        )
        .expect("start");
    assert!(runner.wait_until_idle(Duration::from_secs(15)));

    let matter = Matter::open(&root).expect("open");
    let parent = matter.get_job(&parent_id).expect("parent");
    assert_eq!(
        parent.state,
        JobState::Succeeded,
        "err={:?}",
        parent.error_summary
    );
    assert_eq!(parent.kind, JOB_KIND_WORKFLOW_RUN);

    let children = matter.list_child_jobs(&parent_id).expect("children");
    assert_eq!(children.len(), 2, "expected two child job nodes");
    assert!(children.iter().all(|c| c.state == JobState::Succeeded));
    assert!(children
        .iter()
        .all(|c| c.parent_job_id.as_deref() == Some(&parent_id)));

    let kinds: Vec<_> = children.iter().map(|c| c.kind.as_str()).collect();
    assert_eq!(kinds, vec!["dedupe", "thread"]);

    let cp = matter
        .get_checkpoint(&parent_id, "workflow_run")
        .expect("cp")
        .expect("checkpoint present");
    let cursor: serde_json::Value = serde_json::from_str(&cp.cursor_json).expect("json");
    assert_eq!(cursor["workflow_name"], "two_jobs");
    assert!(cursor["definition_body"].is_object());
    let nodes = cursor["nodes"].as_array().expect("nodes");
    assert_eq!(nodes.len(), 2);
    assert_eq!(nodes[0]["status"], "succeeded");
    assert_eq!(nodes[1]["status"], "succeeded");
}

#[test]
fn workflow_soft_fail_continues_to_next_node() {
    let (_tmp, base) = utf8_tempdir();
    let root = make_matter(&base, "m-wf-soft");

    {
        let matter = Matter::open(&root).expect("open");
        let body = r#"{
            "version": 1,
            "nodes": [
                { "id": "n1", "type": "job", "kind": "gap", "soft_fail": true, "params": {} },
                { "id": "n2", "type": "job", "kind": "dedupe", "params": {} }
            ]
        }"#;
        matter
            .upsert_workflow(WorkflowInput {
                id: None,
                name: "soft_then_ok".into(),
                description: None,
                body_json: body.into(),
                created_by: None,
            })
            .expect("upsert");
    }

    let mut handler = MatterWorkflowRunHandler::new();
    handler.register_node(Arc::new(FailNodeHandler { kind: "gap" }));
    handler.register_node(Arc::new(OkNodeHandler { kind: "dedupe" }));

    let mut runner = ProcessRunner::new(RunnerConfig::default());
    runner.register(Arc::new(handler));

    let parent_id = runner
        .start(
            &root,
            JOB_KIND_WORKFLOW_RUN,
            JobParams::new(r#"{"workflow_name":"soft_then_ok"}"#),
        )
        .expect("start");
    assert!(runner.wait_until_idle(Duration::from_secs(15)));

    let matter = Matter::open(&root).expect("open");
    let parent = matter.get_job(&parent_id).expect("parent");
    assert_eq!(
        parent.state,
        JobState::Succeeded,
        "soft_fail should allow workflow success; err={:?}",
        parent.error_summary
    );

    let children = matter.list_child_jobs(&parent_id).expect("children");
    assert_eq!(children.len(), 2);
    let gap = children.iter().find(|c| c.kind == "gap").expect("gap");
    let dedupe = children
        .iter()
        .find(|c| c.kind == "dedupe")
        .expect("dedupe");
    assert_eq!(gap.state, JobState::Failed);
    assert_eq!(dedupe.state, JobState::Succeeded);

    // Cursor records soft_failed (terminal for resume), not bare "failed".
    let cp = matter
        .get_checkpoint(&parent_id, "workflow_run")
        .expect("cp")
        .expect("checkpoint");
    let cursor: serde_json::Value = serde_json::from_str(&cp.cursor_json).expect("json");
    let nodes = cursor["nodes"].as_array().expect("nodes");
    let gap_entry = nodes
        .iter()
        .find(|n| n["node_id"] == "n1")
        .expect("n1 entry");
    assert_eq!(gap_entry["status"], "soft_failed");
}

#[test]
fn workflow_audit_start_and_complete_include_run_params() {
    let (_tmp, base) = utf8_tempdir();
    let root = make_matter(&base, "m-wf-audit");

    {
        let matter = Matter::open(&root).expect("open");
        let body = r#"{
            "version": 1,
            "nodes": [
                { "id": "n1", "type": "job", "kind": "dedupe", "params": {} }
            ]
        }"#;
        matter
            .upsert_workflow(WorkflowInput {
                id: None,
                name: "audit_chain".into(),
                description: None,
                body_json: body.into(),
                created_by: None,
            })
            .expect("upsert");
    }

    let mut handler = MatterWorkflowRunHandler::new();
    handler.register_node(Arc::new(OkNodeHandler { kind: "dedupe" }));

    let mut runner = ProcessRunner::new(RunnerConfig::default());
    runner.register(Arc::new(handler));

    let params = r#"{
        "workflow_name": "audit_chain",
        "run_params": { "source_path": "C:\\cases\\export" }
    }"#;
    let parent_id = runner
        .start(&root, JOB_KIND_WORKFLOW_RUN, JobParams::new(params))
        .expect("start");
    assert!(runner.wait_until_idle(Duration::from_secs(15)));

    let matter = Matter::open(&root).expect("open");
    let parent = matter.get_job(&parent_id).expect("parent");
    assert_eq!(parent.state, JobState::Succeeded);

    let mut stmt = matter
        .connection()
        .prepare(
            "SELECT action, params_json FROM audit_events \
             WHERE action IN ('workflow_run.start', 'workflow_run.complete') \
             AND entity = ?1 ORDER BY seq ASC",
        )
        .expect("prepare");
    let entity = format!("job:{parent_id}");
    let rows: Vec<(String, String)> = stmt
        .query_map([&entity], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .expect("query")
        .collect::<Result<_, _>>()
        .expect("rows");

    assert!(
        rows.iter().any(|(a, _)| a == "workflow_run.start"),
        "missing workflow_run.start; got {:?}",
        rows.iter().map(|(a, _)| a.as_str()).collect::<Vec<_>>()
    );
    assert!(
        rows.iter().any(|(a, _)| a == "workflow_run.complete"),
        "missing workflow_run.complete; got {:?}",
        rows.iter().map(|(a, _)| a.as_str()).collect::<Vec<_>>()
    );

    let start_params = rows
        .iter()
        .find(|(a, _)| a == "workflow_run.start")
        .map(|(_, p)| p)
        .expect("start params");
    let start_val: serde_json::Value = serde_json::from_str(start_params).expect("json");
    assert_eq!(
        start_val["run_params"]["source_path"].as_str(),
        Some(r"C:\cases\export"),
        "start audit must include run_params: {start_val}"
    );
    assert_eq!(start_val["workflow_name"].as_str(), Some("audit_chain"));
}

#[test]
fn workflow_soft_failed_skipped_on_resume() {
    // Soft-failed node is terminal for that node: cancel after soft_fail, resume
    // must not re-run the soft_failed node (only remaining nodes).
    let (_tmp, base) = utf8_tempdir();
    let root = make_matter(&base, "m-wf-soft-resume");

    {
        let matter = Matter::open(&root).expect("open");
        let body = r#"{
            "version": 1,
            "nodes": [
                { "id": "n1", "type": "job", "kind": "gap", "soft_fail": true, "params": {} },
                { "id": "n2", "type": "job", "kind": "dedupe", "params": {} },
                { "id": "n3", "type": "job", "kind": "thread", "params": {} }
            ]
        }"#;
        matter
            .upsert_workflow(WorkflowInput {
                id: None,
                name: "soft_resume".into(),
                description: None,
                body_json: body.into(),
                created_by: None,
            })
            .expect("upsert");
    }

    let ticks = Arc::new(AtomicUsize::new(0));
    let mut handler = MatterWorkflowRunHandler::new();
    handler.register_node(Arc::new(FailNodeHandler { kind: "gap" }));
    handler.register_node(Arc::new(SlowNodeHandler {
        kind: "dedupe",
        ticks: Arc::clone(&ticks),
    }));
    handler.register_node(Arc::new(OkNodeHandler { kind: "thread" }));

    let mut runner = ProcessRunner::new(RunnerConfig::default());
    runner.register(Arc::new(handler));

    let parent_id = runner
        .start(
            &root,
            JOB_KIND_WORKFLOW_RUN,
            JobParams::new(r#"{"workflow_name":"soft_resume"}"#),
        )
        .expect("start");

    // Wait until slow second node is running (soft_fail gap already finished).
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if ticks.load(Ordering::SeqCst) > 2 {
            break;
        }
        assert!(Instant::now() < deadline, "slow node never ticked");
        thread::sleep(Duration::from_millis(20));
    }
    runner.cancel(&parent_id).expect("cancel");
    assert!(runner.wait_until_idle(Duration::from_secs(15)));

    {
        let matter = Matter::open(&root).expect("open");
        let parent = matter.get_job(&parent_id).expect("parent");
        assert_eq!(parent.state, JobState::Paused);
        let children = matter.list_child_jobs(&parent_id).expect("children");
        assert_eq!(children.len(), 2, "gap + dedupe only");
        let gap = children.iter().find(|c| c.kind == "gap").expect("gap");
        assert_eq!(gap.state, JobState::Failed);
        let cp = matter
            .get_checkpoint(&parent_id, "workflow_run")
            .expect("cp")
            .expect("checkpoint");
        let cursor: serde_json::Value = serde_json::from_str(&cp.cursor_json).expect("json");
        let n1 = cursor["nodes"]
            .as_array()
            .expect("nodes")
            .iter()
            .find(|n| n["node_id"] == "n1")
            .expect("n1");
        assert_eq!(n1["status"], "soft_failed");
    }

    // Resume: gap must not re-run (still one gap child); dedupe + thread succeed.
    let mut handler2 = MatterWorkflowRunHandler::new();
    handler2.register_node(Arc::new(FailNodeHandler { kind: "gap" }));
    handler2.register_node(Arc::new(OkNodeHandler { kind: "dedupe" }));
    handler2.register_node(Arc::new(OkNodeHandler { kind: "thread" }));
    let mut runner2 = ProcessRunner::new(RunnerConfig::default());
    runner2.register(Arc::new(handler2));
    runner2
        .resume(&root, &parent_id)
        .expect("resume workflow_run");
    assert!(runner2.wait_until_idle(Duration::from_secs(15)));

    let matter = Matter::open(&root).expect("reopen");
    let parent = matter.get_job(&parent_id).expect("parent");
    assert_eq!(
        parent.state,
        JobState::Succeeded,
        "err={:?}",
        parent.error_summary
    );
    let children = matter.list_child_jobs(&parent_id).expect("children");
    let gap_count = children.iter().filter(|c| c.kind == "gap").count();
    assert_eq!(
        gap_count, 1,
        "soft_failed gap must not re-run on resume; children={children:?}"
    );
    assert!(children
        .iter()
        .filter(|c| c.kind != "gap")
        .all(|c| c.state == JobState::Succeeded));

    // Resume must not re-emit workflow_run.start.
    let start_count: i64 = matter
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM audit_events \
             WHERE action = 'workflow_run.start' AND entity = ?1",
            [format!("job:{parent_id}")],
            |row| row.get(0),
        )
        .expect("count start");
    assert_eq!(start_count, 1, "workflow_run.start must emit once");
}

#[test]
fn workflow_gate_require_qc_pass_hard_fails() {
    let (_tmp, base) = utf8_tempdir();
    let root = make_matter(&base, "m-wf-qc-gate");

    {
        let matter = Matter::open(&root).expect("open");
        // No QC run → require_qc_pass hard-fails before produce.
        let body = r#"{
            "version": 1,
            "nodes": [
                { "id": "n1", "type": "gate", "kind": "require_qc_pass", "params": {} },
                { "id": "n2", "type": "job", "kind": "produce", "params": {} }
            ]
        }"#;
        matter
            .upsert_workflow(WorkflowInput {
                id: None,
                name: "qc_gate_first".into(),
                description: None,
                body_json: body.into(),
                created_by: None,
            })
            .expect("upsert");
    }

    let mut handler = MatterWorkflowRunHandler::new();
    handler.register_node(Arc::new(OkNodeHandler { kind: "produce" }));

    let mut runner = ProcessRunner::new(RunnerConfig::default());
    runner.register(Arc::new(handler));

    let parent_id = runner
        .start(
            &root,
            JOB_KIND_WORKFLOW_RUN,
            JobParams::new(r#"{"workflow_name":"qc_gate_first"}"#),
        )
        .expect("start");
    assert!(runner.wait_until_idle(Duration::from_secs(15)));

    let matter = Matter::open(&root).expect("open");
    let parent = matter.get_job(&parent_id).expect("parent");
    assert_eq!(
        parent.state,
        JobState::Failed,
        "require_qc_pass must hard-fail; err={:?}",
        parent.error_summary
    );
    let children = matter.list_child_jobs(&parent_id).expect("children");
    assert_eq!(children.len(), 1, "produce must not run after gate fail");
    assert_eq!(children[0].kind, "require_qc_pass");
    assert_eq!(children[0].state, JobState::Failed);
}

#[test]
fn workflow_gate_hard_fails() {
    let (_tmp, base) = utf8_tempdir();
    let root = make_matter(&base, "m-wf-gate");

    {
        let matter = Matter::open(&root).expect("open");
        // require_has_sources fails on empty matter.
        let body = r#"{
            "version": 1,
            "nodes": [
                { "id": "n1", "type": "gate", "kind": "require_has_sources", "params": {} },
                { "id": "n2", "type": "job", "kind": "dedupe", "params": {} }
            ]
        }"#;
        matter
            .upsert_workflow(WorkflowInput {
                id: None,
                name: "gate_first".into(),
                description: None,
                body_json: body.into(),
                created_by: None,
            })
            .expect("upsert");
    }

    let mut handler = MatterWorkflowRunHandler::new();
    handler.register_node(Arc::new(OkNodeHandler { kind: "dedupe" }));

    let mut runner = ProcessRunner::new(RunnerConfig::default());
    runner.register(Arc::new(handler));

    let parent_id = runner
        .start(
            &root,
            JOB_KIND_WORKFLOW_RUN,
            JobParams::new(r#"{"workflow_name":"gate_first"}"#),
        )
        .expect("start");
    assert!(runner.wait_until_idle(Duration::from_secs(15)));

    let matter = Matter::open(&root).expect("open");
    let parent = matter.get_job(&parent_id).expect("parent");
    assert_eq!(
        parent.state,
        JobState::Failed,
        "gate must hard-fail workflow; err={:?}",
        parent.error_summary
    );

    let children = matter.list_child_jobs(&parent_id).expect("children");
    assert_eq!(
        children.len(),
        1,
        "second node must not run after gate fail"
    );
    assert_eq!(children[0].kind, "require_has_sources");
    assert_eq!(children[0].state, JobState::Failed);
    assert_eq!(
        children[0].parent_job_id.as_deref(),
        Some(parent_id.as_str())
    );
}

#[test]
fn workflow_gate_soft_fail_rejected_at_upsert() {
    let (_tmp, base) = utf8_tempdir();
    let root = make_matter(&base, "m-wf-gate-soft");
    let matter = Matter::open(&root).expect("open");
    let body = r#"{
        "version": 1,
        "nodes": [
            { "id": "n1", "type": "gate", "kind": "require_qc_pass", "soft_fail": true, "params": {} }
        ]
    }"#;
    let err = matter
        .upsert_workflow(WorkflowInput {
            id: None,
            name: "bad_gate".into(),
            description: None,
            body_json: body.into(),
            created_by: None,
        })
        .expect_err("soft_fail on gate must be rejected");
    let msg = err.to_string();
    assert!(
        msg.contains("soft_fail") || msg.contains("hard-fail"),
        "unexpected: {msg}"
    );
}

#[test]
fn workflow_parent_job_id_on_children() {
    let (_tmp, base) = utf8_tempdir();
    let root = make_matter(&base, "m-wf-parent");

    {
        let matter = Matter::open(&root).expect("open");
        let body = r#"{
            "version": 1,
            "nodes": [
                { "id": "n1", "type": "job", "kind": "classify", "params": {} },
                { "id": "n2", "type": "job", "kind": "cull", "params": {} }
            ]
        }"#;
        matter
            .upsert_workflow(WorkflowInput {
                id: None,
                name: "parent_check".into(),
                description: None,
                body_json: body.into(),
                created_by: None,
            })
            .expect("upsert");
    }

    let mut handler = MatterWorkflowRunHandler::new();
    handler.register_node(Arc::new(OkNodeHandler { kind: "classify" }));
    handler.register_node(Arc::new(OkNodeHandler { kind: "cull" }));

    let mut runner = ProcessRunner::new(RunnerConfig::default());
    runner.register(Arc::new(handler));

    let parent_id = runner
        .start(
            &root,
            JOB_KIND_WORKFLOW_RUN,
            JobParams::new(r#"{"workflow_id":"parent_check"}"#),
        )
        .expect("start by name as id_or_name");
    // get_workflow accepts name; also try after open.
    assert!(runner.wait_until_idle(Duration::from_secs(15)));

    let matter = Matter::open(&root).expect("open");
    // If start failed because workflow_id looked up as id only — check state.
    let parent = matter.get_job(&parent_id).expect("parent");
    assert_eq!(
        parent.state,
        JobState::Succeeded,
        "err={:?}",
        parent.error_summary
    );

    let children = matter.list_child_jobs(&parent_id).expect("children");
    assert_eq!(children.len(), 2);
    for c in &children {
        assert_eq!(
            c.parent_job_id.as_deref(),
            Some(parent_id.as_str()),
            "child {} missing parent",
            c.id
        );
    }
}

#[test]
fn profile_run_stage_children_have_parent_job_id() {
    let (_tmp, base) = utf8_tempdir();
    let root = make_matter(&base, "m-pr-parent");

    {
        let matter = Matter::open(&root).expect("open");
        let body = r#"{
            "version": 1,
            "stages": {
                "classify": { "enabled": true, "params": {} },
                "office_extract": { "enabled": true, "params": {} }
            }
        }"#;
        matter
            .upsert_processing_profile(ProcessingProfileInput {
                id: None,
                name: "two_stage".into(),
                description: None,
                body_json: body.into(),
                created_by: None,
            })
            .expect("upsert profile");
    }

    let mut handler = MatterProfileRunHandler::new();
    handler.register_stage(Arc::new(OkNodeHandler { kind: "classify" }));
    handler.register_stage(Arc::new(OkNodeHandler {
        kind: "office_extract",
    }));

    let mut runner = ProcessRunner::new(RunnerConfig::default());
    runner.register(Arc::new(handler));

    let parent_id = runner
        .start(
            &root,
            JOB_KIND_PROFILE_RUN,
            JobParams::new(r#"{"profile_name":"two_stage"}"#),
        )
        .expect("start");
    assert!(runner.wait_until_idle(Duration::from_secs(15)));

    let matter = Matter::open(&root).expect("open");
    let parent = matter.get_job(&parent_id).expect("parent");
    assert_eq!(parent.state, JobState::Succeeded);

    let children = matter.list_child_jobs(&parent_id).expect("children");
    assert_eq!(children.len(), 2, "two stage children");
    for c in &children {
        assert_eq!(
            c.parent_job_id.as_deref(),
            Some(parent_id.as_str()),
            "stage child {} parent_job_id",
            c.kind
        );
    }
}

#[test]
fn workflow_cancel_mid_node_pauses_and_resume() {
    let (_tmp, base) = utf8_tempdir();
    let root = make_matter(&base, "m-wf-cancel");

    {
        let matter = Matter::open(&root).expect("open");
        let body = r#"{
            "version": 1,
            "nodes": [
                { "id": "n1", "type": "job", "kind": "dedupe", "params": {} },
                { "id": "n2", "type": "job", "kind": "thread", "params": {} }
            ]
        }"#;
        matter
            .upsert_workflow(WorkflowInput {
                id: None,
                name: "cancel_mid".into(),
                description: None,
                body_json: body.into(),
                created_by: None,
            })
            .expect("upsert");
    }

    let ticks = Arc::new(AtomicUsize::new(0));
    let mut handler = MatterWorkflowRunHandler::new();
    handler.register_node(Arc::new(SlowNodeHandler {
        kind: "dedupe",
        ticks: Arc::clone(&ticks),
    }));
    handler.register_node(Arc::new(OkNodeHandler { kind: "thread" }));

    let mut runner = ProcessRunner::new(RunnerConfig::default());
    runner.register(Arc::new(handler));

    let parent_id = runner
        .start(
            &root,
            JOB_KIND_WORKFLOW_RUN,
            JobParams::new(r#"{"workflow_name":"cancel_mid"}"#),
        )
        .expect("start");

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if ticks.load(Ordering::SeqCst) > 2 {
            break;
        }
        assert!(Instant::now() < deadline, "slow node never ticked");
        thread::sleep(Duration::from_millis(20));
    }
    runner.cancel(&parent_id).expect("cancel");
    assert!(runner.wait_until_idle(Duration::from_secs(15)));

    {
        let matter = Matter::open(&root).expect("open");
        let parent = matter.get_job(&parent_id).expect("parent");
        assert_eq!(
            parent.state,
            JobState::Paused,
            "err={:?}",
            parent.error_summary
        );
        let children = matter.list_child_jobs(&parent_id).expect("children");
        assert_eq!(children.len(), 1, "thread must not start after cancel");
        assert_eq!(children[0].kind, "dedupe");
        assert_eq!(children[0].state, JobState::Paused);
    }

    // Resume with fast ok handlers; reuse paused dedupe child.
    let paused_dedupe = {
        let matter = Matter::open(&root).expect("open");
        matter
            .list_child_jobs(&parent_id)
            .expect("c")
            .into_iter()
            .find(|c| c.kind == "dedupe")
            .expect("dedupe")
            .id
    };

    let mut handler2 = MatterWorkflowRunHandler::new();
    handler2.register_node(Arc::new(OkNodeHandler { kind: "dedupe" }));
    handler2.register_node(Arc::new(OkNodeHandler { kind: "thread" }));
    let mut runner2 = ProcessRunner::new(RunnerConfig::default());
    runner2.register(Arc::new(handler2));
    runner2
        .resume(&root, &parent_id)
        .expect("resume workflow_run");
    assert!(runner2.wait_until_idle(Duration::from_secs(15)));

    let matter = Matter::open(&root).expect("reopen");
    let parent = matter.get_job(&parent_id).expect("parent");
    assert_eq!(
        parent.state,
        JobState::Succeeded,
        "resume err={:?}",
        parent.error_summary
    );
    let children = matter.list_child_jobs(&parent_id).expect("children");
    assert_eq!(children.len(), 2);
    let dedupe = children
        .iter()
        .find(|c| c.kind == "dedupe")
        .expect("dedupe");
    assert_eq!(
        dedupe.id, paused_dedupe,
        "resume must reuse paused child job"
    );
    assert!(children.iter().all(|c| c.state == JobState::Succeeded));
}

#[test]
fn workflow_nested_profile_run_parent_chain() {
    let (_tmp, base) = utf8_tempdir();
    let root = make_matter(&base, "m-wf-nested");

    {
        let matter = Matter::open(&root).expect("open");
        let profile_body = r#"{
            "version": 1,
            "stages": {
                "classify": { "enabled": true, "params": {} }
            }
        }"#;
        matter
            .upsert_processing_profile(ProcessingProfileInput {
                id: None,
                name: "cls_only".into(),
                description: None,
                body_json: profile_body.into(),
                created_by: None,
            })
            .expect("profile");
        let wf_body = r#"{
            "version": 1,
            "nodes": [
                {
                    "id": "n1",
                    "type": "profile_run",
                    "profile": "cls_only",
                    "params": { "stop_on_stage_failure": true }
                }
            ]
        }"#;
        matter
            .upsert_workflow(WorkflowInput {
                id: None,
                name: "nested_pr".into(),
                description: None,
                body_json: wf_body.into(),
                created_by: None,
            })
            .expect("workflow");
    }

    let mut profile = MatterProfileRunHandler::new();
    profile.register_stage(Arc::new(OkNodeHandler { kind: "classify" }));

    let mut workflow = MatterWorkflowRunHandler::new();
    workflow.register_node(Arc::new(profile));

    let mut runner = ProcessRunner::new(RunnerConfig::default());
    runner.register(Arc::new(workflow));

    let wf_job = runner
        .start(
            &root,
            JOB_KIND_WORKFLOW_RUN,
            JobParams::new(r#"{"workflow_name":"nested_pr"}"#),
        )
        .expect("start");
    assert!(runner.wait_until_idle(Duration::from_secs(15)));

    let matter = Matter::open(&root).expect("open");
    let parent = matter.get_job(&wf_job).expect("wf");
    assert_eq!(
        parent.state,
        JobState::Succeeded,
        "err={:?}",
        parent.error_summary
    );

    let wf_children = matter.list_child_jobs(&wf_job).expect("wf children");
    assert_eq!(wf_children.len(), 1);
    assert_eq!(wf_children[0].kind, JOB_KIND_PROFILE_RUN);
    assert_eq!(
        wf_children[0].parent_job_id.as_deref(),
        Some(wf_job.as_str())
    );
    let pr_id = wf_children[0].id.clone();

    let stages = matter.list_child_jobs(&pr_id).expect("stage children");
    assert_eq!(stages.len(), 1);
    assert_eq!(stages[0].kind, "classify");
    assert_eq!(stages[0].parent_job_id.as_deref(), Some(pr_id.as_str()));
    assert_eq!(stages[0].state, JobState::Succeeded);
}

#[test]
fn workflow_audit_scrubs_secrets_and_carries_definition_identity() {
    let (_tmp, base) = utf8_tempdir();
    let root = make_matter(&base, "m-wf-audit-scrub");

    {
        let matter = Matter::open(&root).expect("open");
        let body = r#"{
            "version": 1,
            "nodes": [
                { "id": "n1", "type": "job", "kind": "gap", "params": {} }
            ]
        }"#;
        matter
            .upsert_workflow(WorkflowInput {
                id: None,
                name: "audit_scrub".into(),
                description: None,
                body_json: body.into(),
                created_by: None,
            })
            .expect("upsert");
    }

    let mut handler = MatterWorkflowRunHandler::new();
    handler.register_node(Arc::new(OkNodeHandler { kind: "gap" }));
    let mut runner = ProcessRunner::new(RunnerConfig::default());
    runner.register(Arc::new(handler));

    let params = r#"{
        "workflow_name": "audit_scrub",
        "run_params": {
            "source_path": "C:\\x",
            "token": "sekrit"
        }
    }"#;
    let parent_id = runner
        .start(&root, JOB_KIND_WORKFLOW_RUN, JobParams::new(params))
        .expect("start");
    assert!(runner.wait_until_idle(Duration::from_secs(15)));

    let matter = Matter::open(&root).expect("open");
    let parent = matter.get_job(&parent_id).expect("parent");
    assert_eq!(
        parent.state,
        JobState::Succeeded,
        "err={:?}",
        parent.error_summary
    );

    // Checkpoint keeps full run_params for resume (paths needed).
    let cp = matter
        .get_checkpoint(&parent_id, "workflow_run")
        .expect("cp")
        .expect("checkpoint present");
    assert!(
        cp.cursor_json.contains("C:\\\\x") || cp.cursor_json.contains(r"C:\\x"),
        "checkpoint should retain source_path; got {}",
        cp.cursor_json
    );
    assert!(
        cp.cursor_json.contains("sekrit"),
        "checkpoint keeps full run_params for resume bind"
    );

    let start_params: String = matter
        .connection()
        .query_row(
            "SELECT params_json FROM audit_events \
             WHERE action = 'workflow_run.start' AND entity = ?1 LIMIT 1",
            [format!("job:{parent_id}")],
            |row| row.get(0),
        )
        .expect("start audit");
    let v: serde_json::Value = serde_json::from_str(&start_params).expect("json");
    assert_eq!(v["run_params"]["source_path"], "C:\\x");
    assert_eq!(v["run_params"]["token"], "[redacted]");
    assert_eq!(v["definition_version"], 1);
    let hash = v["definition_hash"]
        .as_str()
        .expect("definition_hash string");
    assert!(!hash.is_empty(), "definition_hash must be non-empty");
    assert_eq!(hash.len(), 64);
}

#[test]
fn workflow_disabled_node_recorded_not_executed() {
    let (_tmp, base) = utf8_tempdir();
    let root = make_matter(&base, "m-wf-disabled");

    {
        let matter = Matter::open(&root).expect("open");
        let body = r#"{
            "version": 1,
            "nodes": [
                { "id": "n1", "type": "job", "kind": "dedupe", "enabled": true, "params": {} },
                { "id": "n2", "type": "job", "kind": "thread", "enabled": false, "params": {} }
            ]
        }"#;
        matter
            .upsert_workflow(WorkflowInput {
                id: None,
                name: "with_disabled".into(),
                description: None,
                body_json: body.into(),
                created_by: None,
            })
            .expect("upsert");
    }

    let mut handler = MatterWorkflowRunHandler::new();
    handler.register_node(Arc::new(OkNodeHandler { kind: "dedupe" }));
    handler.register_node(Arc::new(OkNodeHandler { kind: "thread" }));
    let mut runner = ProcessRunner::new(RunnerConfig::default());
    runner.register(Arc::new(handler));

    let parent_id = runner
        .start(
            &root,
            JOB_KIND_WORKFLOW_RUN,
            JobParams::new(r#"{"workflow_name":"with_disabled"}"#),
        )
        .expect("start");
    assert!(runner.wait_until_idle(Duration::from_secs(15)));

    let matter = Matter::open(&root).expect("open");
    let parent = matter.get_job(&parent_id).expect("parent");
    assert_eq!(
        parent.state,
        JobState::Succeeded,
        "err={:?}",
        parent.error_summary
    );

    // Only the enabled node creates a child job.
    let children = matter.list_child_jobs(&parent_id).expect("children");
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].kind, "dedupe");
    assert_eq!(children[0].state, JobState::Succeeded);

    let complete_params: String = matter
        .connection()
        .query_row(
            "SELECT params_json FROM audit_events \
             WHERE action = 'workflow_run.complete' AND entity = ?1 LIMIT 1",
            [format!("job:{parent_id}")],
            |row| row.get(0),
        )
        .expect("complete audit");
    let v: serde_json::Value = serde_json::from_str(&complete_params).expect("json");
    let nodes = v["nodes"].as_array().expect("nodes array");
    assert!(
        nodes
            .iter()
            .any(|n| n.as_str().is_some_and(|s| s.contains("disabled"))),
        "planned labels should mark disabled node; got {nodes:?}"
    );
    let outcomes = v["node_outcomes"].as_array().expect("node_outcomes");
    let disabled = outcomes
        .iter()
        .find(|o| o["node_id"] == "n2")
        .expect("disabled node in outcomes");
    assert_eq!(disabled["status"], "disabled");
    assert_eq!(disabled["kind"], "thread");
}

#[test]
fn workflow_builtin_reduce_only_chain_default_handlers() {
    let (_tmp, base) = utf8_tempdir();
    let root = make_matter(&base, "m-wf-builtin-reduce");

    {
        let matter = Matter::open(&root).expect("open");
        let w = matter
            .get_workflow("builtin:reduce_only_chain")
            .expect("resolve builtin");
        assert!(w.is_builtin);
        assert_eq!(w.name, "reduce_only_chain");
    }

    let mut runner = ProcessRunner::new(RunnerConfig::default());
    runner.register(Arc::new(MatterWorkflowRunHandler::with_default_handlers()));

    let parent_id = runner
        .start(
            &root,
            JOB_KIND_WORKFLOW_RUN,
            JobParams::new(r#"{"workflow_id":"builtin:reduce_only_chain"}"#),
        )
        .expect("start");
    // Reduce-only stages on empty matter should skip/idempotent-succeed.
    assert!(runner.wait_until_idle(Duration::from_secs(60)));

    let matter = Matter::open(&root).expect("open");
    let parent = matter.get_job(&parent_id).expect("parent");
    assert_eq!(
        parent.state,
        JobState::Succeeded,
        "builtin reduce_only_chain should succeed on empty matter; err={:?}",
        parent.error_summary
    );

    let children = matter.list_child_jobs(&parent_id).expect("children");
    assert!(
        !children.is_empty(),
        "expect at least profile_run child; got {children:?}"
    );
    assert!(
        children.iter().any(|c| {
            c.kind == JOB_KIND_PROFILE_RUN && c.parent_job_id.as_deref() == Some(parent_id.as_str())
        }),
        "expect profile_run child with parent_job_id; children={children:?}"
    );
}
