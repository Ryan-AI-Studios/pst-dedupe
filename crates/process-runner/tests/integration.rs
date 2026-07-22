//! Integration tests for process-runner.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use camino::Utf8PathBuf;
use matter_core::{JobState, Matter};
use process_runner::{
    JobContext, JobHandler, JobOutcome, JobParams, ProcessRunner, RunnerConfig, RunnerError,
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

/// Fast mock that succeeds immediately.
struct SucceedHandler;

impl JobHandler for SucceedHandler {
    fn kind(&self) -> &'static str {
        "mock_ok"
    }

    fn run(&self, ctx: &JobContext<'_>) -> Result<JobOutcome, RunnerError> {
        ctx.progress.patch(|s| {
            s.completed_count = 1;
            s.message = Some("mock done".into());
        });
        // Stage-style: set durable Succeeded ourselves so finalize trusts it.
        ctx.matter
            .set_job_state(ctx.job_id, JobState::Succeeded, None)
            .map_err(RunnerError::from)?;
        Ok(JobOutcome::Succeeded {
            message: Some("ok".into()),
            completed_count: 1,
        })
    }
}

/// Mock that polls cancel and pauses.
struct SlowCancelHandler {
    ticks: Arc<AtomicUsize>,
}

impl JobHandler for SlowCancelHandler {
    fn kind(&self) -> &'static str {
        "mock_slow"
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
            message: Some("finished without cancel".into()),
            completed_count: self.ticks.load(Ordering::SeqCst) as u64,
        })
    }
}

/// Mock that pauses on first run and succeeds on resume (same job_id).
struct ResumeAwareHandler {
    runs: Arc<AtomicUsize>,
}

impl JobHandler for ResumeAwareHandler {
    fn kind(&self) -> &'static str {
        "mock_resume"
    }

    fn run(&self, ctx: &JobContext<'_>) -> Result<JobOutcome, RunnerError> {
        let n = self.runs.fetch_add(1, Ordering::SeqCst);
        if !ctx.is_resume && n == 0 {
            // Persist a tiny checkpoint-like cursor for source_id.
            let cursor = serde_json::json!({ "source_id": "src-test", "step": 1 });
            ctx.matter
                .put_checkpoint(ctx.job_id, "expand", &cursor.to_string(), 1)
                .map_err(RunnerError::from)?;
            ctx.matter
                .set_job_state(ctx.job_id, JobState::Paused, Some("midway"))
                .map_err(RunnerError::from)?;
            return Ok(JobOutcome::Paused {
                message: Some("midway".into()),
                completed_count: 1,
            });
        }
        ctx.matter
            .set_job_state(ctx.job_id, JobState::Succeeded, None)
            .map_err(RunnerError::from)?;
        Ok(JobOutcome::Succeeded {
            message: Some("resumed ok".into()),
            completed_count: 2,
        })
    }
}

#[test]
fn start_succeeds_exactly_one_job_row() {
    let (_tmp, base) = utf8_tempdir();
    let root = make_matter(&base, "m-ok");

    let mut runner = ProcessRunner::new(RunnerConfig::default());
    runner.register(Arc::new(SucceedHandler));

    let job_id = runner
        .start(&root, "mock_ok", JobParams::empty())
        .expect("start");
    assert!(runner.wait_until_idle(Duration::from_secs(5)));

    let matter = Matter::open(&root).expect("open");
    let jobs = matter.list_jobs().expect("list");
    assert_eq!(jobs.len(), 1, "exactly one job row");
    assert_eq!(jobs[0].id, job_id);
    assert_eq!(jobs[0].state, JobState::Succeeded);
    assert_eq!(jobs[0].kind, "mock_ok");
}

#[test]
fn cancel_mid_run_pauses() {
    let (_tmp, base) = utf8_tempdir();
    let root = make_matter(&base, "m-cancel");
    let ticks = Arc::new(AtomicUsize::new(0));

    let mut runner = ProcessRunner::new(RunnerConfig::default());
    runner.register(Arc::new(SlowCancelHandler {
        ticks: Arc::clone(&ticks),
    }));

    let job_id = runner
        .start(&root, "mock_slow", JobParams::empty())
        .expect("start");

    // Wait until handler is ticking, then cancel.
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while ticks.load(Ordering::SeqCst) == 0 && std::time::Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    runner.cancel(&job_id).expect("cancel");
    assert!(runner.wait_until_idle(Duration::from_secs(5)));

    let matter = Matter::open(&root).expect("open");
    let job = matter.get_job(&job_id).expect("job");
    assert_eq!(job.state, JobState::Paused);

    let snap = runner.watch_progress().borrow().clone();
    assert_eq!(snap.job_id, job_id);
    assert_eq!(snap.state, "paused");
}

#[test]
fn resume_continues_same_job_id() {
    let (_tmp, base) = utf8_tempdir();
    let root = make_matter(&base, "m-resume");
    let runs = Arc::new(AtomicUsize::new(0));

    let mut runner = ProcessRunner::new(RunnerConfig::default());
    runner.register(Arc::new(ResumeAwareHandler {
        runs: Arc::clone(&runs),
    }));

    let job_id = runner
        .start(&root, "mock_resume", JobParams::empty())
        .expect("start");
    assert!(runner.wait_until_idle(Duration::from_secs(5)));

    {
        let matter = Matter::open(&root).expect("open");
        assert_eq!(
            matter.get_job(&job_id).expect("job").state,
            JobState::Paused
        );
    }

    runner.resume(&root, &job_id).expect("resume");
    assert!(runner.wait_until_idle(Duration::from_secs(5)));

    let matter = Matter::open(&root).expect("open");
    let jobs = matter.list_jobs().expect("list");
    assert_eq!(jobs.len(), 1, "resume must not create a second job");
    assert_eq!(jobs[0].id, job_id);
    assert_eq!(jobs[0].state, JobState::Succeeded);
    assert!(runs.load(Ordering::SeqCst) >= 2);
}

#[test]
fn second_start_while_running_is_busy() {
    let (_tmp, base) = utf8_tempdir();
    let root = make_matter(&base, "m-busy");
    let ticks = Arc::new(AtomicUsize::new(0));

    let mut runner = ProcessRunner::new(RunnerConfig::default());
    runner.register(Arc::new(SlowCancelHandler {
        ticks: Arc::clone(&ticks),
    }));
    runner.register(Arc::new(SucceedHandler));

    let job_id = runner
        .start(&root, "mock_slow", JobParams::empty())
        .expect("start");

    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while ticks.load(Ordering::SeqCst) == 0 && std::time::Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }

    let err = runner
        .start(&root, "mock_ok", JobParams::empty())
        .expect_err("must be busy");
    match err {
        RunnerError::Busy { job_id: busy_id } => assert_eq!(busy_id, job_id),
        other => panic!("expected Busy, got {other}"),
    }

    runner.cancel(&job_id).expect("cancel");
    assert!(runner.wait_until_idle(Duration::from_secs(5)));
}

#[test]
fn unknown_kind_errors() {
    let (_tmp, base) = utf8_tempdir();
    let root = make_matter(&base, "m-unknown");

    let runner = ProcessRunner::new(RunnerConfig::default());
    let err = runner
        .start(&root, "no_such_kind", JobParams::empty())
        .expect_err("unknown");
    match err {
        RunnerError::UnknownKind(k) => assert_eq!(k, "no_such_kind"),
        other => panic!("expected UnknownKind, got {other}"),
    }
}

#[test]
fn resume_missing_job_is_job_not_found_not_worker_gone() {
    let (_tmp, base) = utf8_tempdir();
    let root = make_matter(&base, "m-missing");

    let mut runner = ProcessRunner::new(RunnerConfig::default());
    runner.register(Arc::new(SucceedHandler));

    let err = runner
        .resume(&root, "job_does_not_exist")
        .expect_err("missing job");
    match err {
        RunnerError::JobNotFound(id) => assert_eq!(id, "job_does_not_exist"),
        other => panic!("expected JobNotFound, got {other:?} (must not be WorkerGone)"),
    }
}

/// Mock that writes expanding checkpoints while sleeping so the mid-run poller
/// can surface `completed_count` before terminal.
struct CheckpointingSlowHandler {
    ticks: Arc<AtomicUsize>,
}

impl JobHandler for CheckpointingSlowHandler {
    fn kind(&self) -> &'static str {
        "mock_cp"
    }

    fn run(&self, ctx: &JobContext<'_>) -> Result<JobOutcome, RunnerError> {
        for i in 1..=5u64 {
            if ctx.cancel.is_cancelled() {
                ctx.matter
                    .set_job_state(ctx.job_id, JobState::Paused, Some("cancelled"))
                    .map_err(RunnerError::from)?;
                return Ok(JobOutcome::Paused {
                    message: Some("cancelled".into()),
                    completed_count: i,
                });
            }
            let cursor = serde_json::json!({ "source_id": "src-cp", "step": i });
            ctx.matter
                .put_checkpoint(ctx.job_id, "expand", &cursor.to_string(), i as i64)
                .map_err(RunnerError::from)?;
            self.ticks.fetch_add(1, Ordering::SeqCst);
            thread::sleep(Duration::from_millis(80));
        }
        ctx.matter
            .set_job_state(ctx.job_id, JobState::Succeeded, None)
            .map_err(RunnerError::from)?;
        Ok(JobOutcome::Succeeded {
            message: Some("done".into()),
            completed_count: 5,
        })
    }
}

#[test]
fn mid_run_watch_reflects_checkpoint_progress() {
    let (_tmp, base) = utf8_tempdir();
    let root = make_matter(&base, "m-mid-progress");
    let ticks = Arc::new(AtomicUsize::new(0));

    let mut runner = ProcessRunner::new(RunnerConfig::default());
    runner.register(Arc::new(CheckpointingSlowHandler {
        ticks: Arc::clone(&ticks),
    }));
    let mut rx = runner.watch_progress();

    let job_id = runner
        .start(&root, "mock_cp", JobParams::empty())
        .expect("start");

    // Wait until at least one checkpoint is visible via watch (mid-run).
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let mut saw_mid = false;
    while std::time::Instant::now() < deadline {
        let snap = rx.borrow_and_update().clone();
        if snap.job_id == job_id
            && snap.state == "running"
            && snap.completed_count >= 1
            && snap.stage.as_deref() == Some("expand")
        {
            saw_mid = true;
            break;
        }
        let _ = rx.has_changed();
        thread::sleep(Duration::from_millis(20));
    }
    assert!(
        saw_mid,
        "expected mid-run checkpoint progress on watch, last={:?}",
        rx.borrow().clone()
    );

    assert!(runner.wait_until_idle(Duration::from_secs(5)));
    assert!(ticks.load(Ordering::SeqCst) >= 1);
}

#[test]
fn drop_joins_without_hang() {
    let (_tmp, base) = utf8_tempdir();
    let root = make_matter(&base, "m-drop");
    let ticks = Arc::new(AtomicUsize::new(0));

    let mut runner = ProcessRunner::new(RunnerConfig::default());
    runner.register(Arc::new(SlowCancelHandler {
        ticks: Arc::clone(&ticks),
    }));

    let _job_id = runner
        .start(&root, "mock_slow", JobParams::empty())
        .expect("start");

    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while ticks.load(Ordering::SeqCst) == 0 && std::time::Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }

    // Drop must cancel + join promptly (no hang).
    let start = std::time::Instant::now();
    drop(runner);
    assert!(
        start.elapsed() < Duration::from_secs(10),
        "Drop should join within 10s"
    );
}

#[test]
fn watch_shows_terminal_snapshot() {
    let (_tmp, base) = utf8_tempdir();
    let root = make_matter(&base, "m-watch");

    let mut runner = ProcessRunner::new(RunnerConfig::default());
    runner.register(Arc::new(SucceedHandler));
    let mut rx = runner.watch_progress();

    let job_id = runner
        .start(&root, "mock_ok", JobParams::empty())
        .expect("start");
    assert!(runner.wait_until_idle(Duration::from_secs(5)));

    // Drain until terminal.
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    loop {
        let snap = rx.borrow_and_update().clone();
        if snap.job_id == job_id && snap.is_terminal() {
            assert_eq!(snap.state, "succeeded");
            break;
        }
        if std::time::Instant::now() > deadline {
            panic!("no terminal snapshot: {:?}", rx.borrow().clone());
        }
        let _ = rx.has_changed();
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn handler_runs_on_worker_not_caller() {
    let (_tmp, base) = utf8_tempdir();
    let root = make_matter(&base, "m-thread");

    struct ThreadCheckHandler {
        worker_name: Arc<std::sync::Mutex<String>>,
    }

    impl JobHandler for ThreadCheckHandler {
        fn kind(&self) -> &'static str {
            "mock_thread"
        }

        fn run(&self, ctx: &JobContext<'_>) -> Result<JobOutcome, RunnerError> {
            let name = thread::current().name().unwrap_or("").to_string();
            *self.worker_name.lock().expect("lock") = name;
            ctx.matter
                .set_job_state(ctx.job_id, JobState::Succeeded, None)
                .map_err(RunnerError::from)?;
            Ok(JobOutcome::Succeeded {
                message: None,
                completed_count: 0,
            })
        }
    }

    let worker_name = Arc::new(std::sync::Mutex::new(String::new()));
    let mut runner = ProcessRunner::new(RunnerConfig::default());
    runner.register(Arc::new(ThreadCheckHandler {
        worker_name: Arc::clone(&worker_name),
    }));

    let caller = thread::current().name().unwrap_or("").to_string();
    let _ = runner
        .start(&root, "mock_thread", JobParams::empty())
        .expect("start");
    assert!(runner.wait_until_idle(Duration::from_secs(5)));

    let name = worker_name.lock().expect("lock").clone();
    assert_eq!(name, "matter-worker");
    assert_ne!(name, caller);
}

#[cfg(feature = "extract_pst")]
#[test]
fn extract_pst_fixture_via_runner() {
    use extract_pst::JOB_KIND_EXTRACT_PST;
    use matter_core::{item_status, ItemInput};
    use process_runner::ExtractPstHandler;
    use std::fs;
    use std::path::PathBuf;

    fn workspace_root() -> PathBuf {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.pop();
        path.pop();
        path
    }

    let root_ws = workspace_root();
    let candidates = [
        root_ws.join("fixtures/aspose_outlook.pst"),
        root_ws.join("fixtures/aspose_sub.pst"),
        root_ws.join("fixtures/sample.pst"),
    ];
    let pst = candidates
        .into_iter()
        .find(|p| p.is_file())
        .expect("repo fixtures required for extract_pst_fixture_via_runner");

    let (_tmp, base) = utf8_tempdir();
    let matter_root = make_matter(&base, "m-extract");
    let (source_id, inv_id) = {
        let matter = Matter::open(&matter_root).expect("open");
        let source = matter
            .insert_source(pst.to_str().unwrap(), "pst", "importing", None)
            .expect("source");
        // Stream into CAS without holding full file if large — fixtures are small.
        let bytes = fs::read(&pst).expect("read");
        let digest = matter.put_bytes(&bytes).expect("cas");
        let name = pst
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("mail.pst");
        // Inventory path is leaf name; open uses CAS digest (extract-pst open resolve).
        let inv = matter
            .insert_item(ItemInput {
                source_id: Some(source.id.clone()),
                path: Some(name.into()),
                native_sha256: Some(digest),
                status: item_status::DISCOVERED.to_string(),
                size_bytes: Some(bytes.len() as i64),
                file_category: Some("pst".into()),
                ..Default::default()
            })
            .expect("inv");
        (source.id, inv.id)
    };

    let mut runner = ProcessRunner::new(RunnerConfig::default());
    runner.register(Arc::new(ExtractPstHandler::new()));

    let params = JobParams::new(
        serde_json::json!({
            "source_id": source_id,
            "pst_item_id": inv_id,
            "limits": { "batch_size": 10, "max_messages": 5 }
        })
        .to_string(),
    );
    let job_id = runner
        .start(&matter_root, JOB_KIND_EXTRACT_PST, params)
        .expect("start extract");
    assert!(runner.wait_until_idle(Duration::from_secs(60)));

    let matter = Matter::open(&matter_root).expect("open");
    let jobs = matter.list_jobs().expect("list");
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].id, job_id);
    // max_messages may Pause; success is also fine. Failure is not acceptable for known fixtures.
    assert!(
        matches!(jobs[0].state, JobState::Succeeded | JobState::Paused),
        "extract must not Fail on known fixture; state={:?} err={:?}",
        jobs[0].state,
        jobs[0].error_summary
    );
    // Prefer path form for open identity if CAS-only fails on some fixtures:
    // also assert we wrote at least a checkpoint or messages when Succeeded.
    if jobs[0].state == JobState::Paused {
        let cp = matter
            .get_checkpoint(&job_id, "pst_extract")
            .expect("cp")
            .expect("paused extract must have checkpoint");
        assert!(cp.completed_count >= 0);
    }
}

#[cfg(feature = "ingest")]
#[test]
fn ingest_zip_via_runner_one_job() {
    use process_runner::IngestHandler;
    use std::fs::File;
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    use zip::{CompressionMethod, ZipWriter};

    let (_tmp, base) = utf8_tempdir();
    let zip_path = base.join("pkg.zip");
    {
        let file = File::create(zip_path.as_std_path()).expect("zip");
        let mut zip = ZipWriter::new(file);
        let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        zip.start_file("hello.txt", opts).expect("start");
        zip.write_all(b"hi").expect("write");
        zip.finish().expect("finish");
    }

    let matter_root = make_matter(&base, "m-ingest");
    let mut runner = ProcessRunner::new(RunnerConfig::default());
    runner.register(Arc::new(IngestHandler::new()));

    let params = JobParams::new(
        serde_json::json!({
            "path": zip_path.as_str(),
            "limits": { "checkpoint_every_n_entries": 1, "max_entries": 1000 }
        })
        .to_string(),
    );
    let job_id = runner
        .start(&matter_root, "ingest", params)
        .expect("start ingest");
    assert!(runner.wait_until_idle(Duration::from_secs(30)));

    let matter = Matter::open(&matter_root).expect("open");
    let jobs = matter.list_jobs().expect("list");
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].id, job_id);
    assert_eq!(jobs[0].state, JobState::Succeeded);

    let snap = runner.watch_progress().borrow().clone();
    assert_eq!(snap.state, "succeeded");
    assert_eq!(snap.job_id, job_id);
}

/// Real ingest handler: cancel → Paused + checkpoint → resume same job_id → Succeeded.
#[cfg(feature = "ingest")]
#[test]
fn ingest_cancel_then_resume_same_job() {
    use process_runner::IngestHandler;
    use std::fs::File;
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    use zip::{CompressionMethod, ZipWriter};

    let (_tmp, base) = utf8_tempdir();
    let zip_path = base.join("big.zip");
    {
        let file = File::create(zip_path.as_std_path()).expect("zip");
        let mut zip = ZipWriter::new(file);
        let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        // Many entries so cancel can land mid-expand (slow enough under debug).
        for i in 0..400 {
            zip.start_file(format!("f{i:03}.txt"), opts).expect("start");
            // Slightly larger payload to stretch expand time.
            zip.write_all(&[b'x'; 256]).expect("write");
        }
        zip.finish().expect("finish");
    }

    let matter_root = make_matter(&base, "m-ingest-resume");
    let mut runner = ProcessRunner::new(RunnerConfig::default());
    runner.register(Arc::new(IngestHandler::new()));

    let params = JobParams::new(
        serde_json::json!({
            "path": zip_path.as_str(),
            "limits": {
                "checkpoint_every_n_entries": 1,
                "max_entries": 10_000
            }
        })
        .to_string(),
    );

    let job_id = runner
        .start(&matter_root, "ingest", params)
        .expect("start ingest");

    // Wait until active, then cancel. Prefer durable Paused path over success race.
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    while !runner.is_busy() && std::time::Instant::now() < deadline {
        thread::sleep(Duration::from_millis(2));
    }
    // Cancel repeatedly while busy so the token is set as soon as ActiveJob exists.
    while runner.is_busy() && std::time::Instant::now() < deadline {
        let _ = runner.cancel(&job_id);
        thread::sleep(Duration::from_millis(5));
    }
    assert!(runner.wait_until_idle(Duration::from_secs(60)));

    let matter = Matter::open(&matter_root).expect("open");
    let job = matter.get_job(&job_id).expect("job");
    assert_eq!(matter.list_jobs().expect("list").len(), 1);

    // If cancel lost the race entirely, re-run is still one Succeeded job — fail the
    // test so we notice (DoD-8 requires cancel→Paused proof on a real handler).
    assert_eq!(
        job.state,
        JobState::Paused,
        "expected Paused after cancel, got {:?} err={:?}; enlarge fixture if flaky",
        job.state,
        job.error_summary
    );
    let _cp = matter
        .get_checkpoint(&job_id, "expand")
        .expect("cp")
        .expect("paused ingest must have expand checkpoint");

    drop(matter);
    runner.resume(&matter_root, &job_id).expect("resume");
    assert!(runner.wait_until_idle(Duration::from_secs(120)));

    let matter = Matter::open(&matter_root).expect("open");
    let jobs = matter.list_jobs().expect("list");
    assert_eq!(jobs.len(), 1, "resume must not create a second job");
    assert_eq!(jobs[0].id, job_id);
    assert_eq!(
        jobs[0].state,
        JobState::Succeeded,
        "resume should finish: {:?}",
        jobs[0].error_summary
    );
}

#[test]
fn durable_running_job_blocks_second_start() {
    let (_tmp, base) = utf8_tempdir();
    let root = make_matter(&base, "m-durable-busy");

    // Simulate a prior crash: job row left Running with no live worker.
    {
        let matter = Matter::open(&root).expect("open");
        let job = matter.create_job("mock_ok").expect("create");
        matter
            .set_job_state(&job.id, JobState::Running, None)
            .expect("running");
    }

    let mut runner = ProcessRunner::new(RunnerConfig::default());
    runner.register(Arc::new(SucceedHandler));
    let err = runner
        .start(&root, "mock_ok", JobParams::empty())
        .expect_err("must reject durable Running");
    match err {
        RunnerError::Busy { .. } => {}
        other => panic!("expected Busy, got {other}"),
    }
}

#[cfg(feature = "dedupe")]
#[test]
fn dedupe_via_runner_one_job_row() {
    use matter_core::{item_dedup_role, item_role, item_status, ItemInput};
    use process_runner::MatterDedupeHandler;

    let (_tmp, base) = utf8_tempdir();
    let root = make_matter(&base, "m-dedupe");

    {
        let matter = Matter::open(&root).expect("open");
        matter
            .insert_item(ItemInput {
                status: item_status::EXTRACTED.into(),
                role: Some(item_role::PARENT.into()),
                file_category: Some("email".into()),
                path: Some("a".into()),
                message_id: Some("same@ex.com".into()),
                ..Default::default()
            })
            .expect("a");
        matter
            .insert_item(ItemInput {
                status: item_status::EXTRACTED.into(),
                role: Some(item_role::PARENT.into()),
                file_category: Some("email".into()),
                path: Some("b".into()),
                message_id: Some("same@ex.com".into()),
                ..Default::default()
            })
            .expect("b");
    }

    let mut runner = ProcessRunner::new(RunnerConfig::default());
    runner.register(Arc::new(MatterDedupeHandler::new()));

    let params = JobParams::new(
        serde_json::json!({
            "use_message_id": true,
            "use_logical_hash": true,
            "family_policy": "suppress_children_with_parent",
            "reset": true,
            "batch_size": 10
        })
        .to_string(),
    );
    let job_id = runner.start(&root, "dedupe", params).expect("start dedupe");
    assert!(runner.wait_until_idle(Duration::from_secs(30)));

    let matter = Matter::open(&root).expect("open");
    let jobs = matter.list_jobs().expect("list");
    assert_eq!(jobs.len(), 1, "Option C: exactly one job row");
    assert_eq!(jobs[0].id, job_id);
    assert_eq!(jobs[0].kind, "dedupe");
    assert_eq!(jobs[0].state, JobState::Succeeded);

    let counts = matter.count_by_dedup_role().expect("counts");
    assert_eq!(counts.unique, 1);
    assert_eq!(counts.duplicate, 1);

    let parents = matter.list_email_parents_for_dedupe().expect("parents");
    let uniques: Vec<_> = parents
        .iter()
        .filter(|p| p.dedup_role.as_deref() == Some(item_dedup_role::UNIQUE))
        .collect();
    assert_eq!(uniques.len(), 1);

    let cp = matter
        .get_checkpoint(&job_id, "dedupe")
        .expect("cp")
        .expect("dedupe checkpoint");
    assert!(cp.completed_count >= 2);
}

#[cfg(feature = "thread")]
#[test]
fn thread_via_runner_one_job_row() {
    use matter_core::{item_role, item_status, item_thread_method, ItemInput};
    use process_runner::MatterThreadHandler;

    let (_tmp, base) = utf8_tempdir();
    let root = make_matter(&base, "m-thread");

    {
        let matter = Matter::open(&root).expect("open");
        matter
            .insert_item(ItemInput {
                status: item_status::EXTRACTED.into(),
                role: Some(item_role::PARENT.into()),
                file_category: Some("email".into()),
                path: Some("b".into()),
                message_id: Some("b@ex.com".into()),
                subject: Some("Hello".into()),
                ..Default::default()
            })
            .expect("b");
        matter
            .insert_item(ItemInput {
                status: item_status::EXTRACTED.into(),
                role: Some(item_role::PARENT.into()),
                file_category: Some("email".into()),
                path: Some("a".into()),
                message_id: Some("a@ex.com".into()),
                in_reply_to: Some("b@ex.com".into()),
                subject: Some("Re: Hello".into()),
                ..Default::default()
            })
            .expect("a");
    }

    let mut runner = ProcessRunner::new(RunnerConfig::default());
    runner.register(Arc::new(MatterThreadHandler::new()));

    let params = JobParams::new(
        serde_json::json!({
            "use_headers": true,
            "use_subject_fallback": true,
            "use_conversation_index": true,
            "reset": true,
            "batch_size": 10,
            "family_inherit": true
        })
        .to_string(),
    );
    let job_id = runner.start(&root, "thread", params).expect("start thread");
    assert!(runner.wait_until_idle(Duration::from_secs(30)));

    let matter = Matter::open(&root).expect("open");
    let jobs = matter.list_jobs().expect("list");
    assert_eq!(jobs.len(), 1, "Option C: exactly one job row");
    assert_eq!(jobs[0].id, job_id);
    assert_eq!(jobs[0].kind, "thread");
    assert_eq!(jobs[0].state, JobState::Succeeded);

    let parents = matter.list_email_parents_for_thread().expect("parents");
    assert_eq!(parents.len(), 2);
    assert_eq!(parents[0].thread_id, parents[1].thread_id);
    assert!(parents[0].thread_id.is_some());

    let a = matter
        .get_item(
            &parents
                .iter()
                .find(|p| p.path.as_deref() == Some("a"))
                .unwrap()
                .id,
        )
        .unwrap();
    assert_eq!(
        a.thread_method.as_deref(),
        Some(item_thread_method::HEADERS)
    );

    let cp = matter
        .get_checkpoint(&job_id, "thread")
        .expect("cp")
        .expect("thread checkpoint");
    assert!(cp.completed_count >= 2);
}

#[cfg(feature = "neardup")]
#[test]
fn neardup_via_runner_one_job_row() {
    use matter_core::{item_near_dup_role, item_role, item_status, ItemInput};
    use process_runner::MatterNearDupHandler;

    let (_tmp, base) = utf8_tempdir();
    let root = make_matter(&base, "m-neardup");

    {
        let matter = Matter::open(&root).expect("open");
        // Shared long boilerplate so a one-word edit stays above threshold 0.80.
        let shared = " alpha bravo charlie delta echo foxtrot golf hotel india juliet \
kilo lima mike november oscar papa quebec romeo sierra tango uniform victor whiskey \
xray yankee zulu one two three four five six seven eight nine ten eleven twelve \
thirteen fourteen fifteen sixteen seventeen eighteen nineteen twenty twentyone \
twentytwo twentythree twentyfour twentyfive twentysix twentyseven twentyeight \
twentynine thirty thirtyone thirtytwo thirtythree thirtyfour thirtyfive thirtysix \
thirtyseven thirtyeight thirtynine forty fortyone fortytwo fortythree fortyfour \
fortyfive fortysix fortyseven fortyeight fortynine fifty";
        let body_a = format!(
            "the quick brown fox jumps over the lazy dog while reviewing the contract draft carefully with counsel present{shared}"
        );
        let body_b = format!(
            "the quick brown fox jumps over the lazy dog while reviewing the contract draft carefully with lawyers present{shared}"
        );
        let da = matter.put_bytes(body_a.as_bytes()).expect("cas a");
        let db = matter.put_bytes(body_b.as_bytes()).expect("cas b");
        matter
            .insert_item(ItemInput {
                status: item_status::EXTRACTED.into(),
                role: Some(item_role::STANDALONE.into()),
                path: Some("a.txt".into()),
                text_sha256: Some(da),
                ..Default::default()
            })
            .expect("a");
        matter
            .insert_item(ItemInput {
                status: item_status::EXTRACTED.into(),
                role: Some(item_role::STANDALONE.into()),
                path: Some("b.txt".into()),
                text_sha256: Some(db),
                ..Default::default()
            })
            .expect("b");
    }

    let mut runner = ProcessRunner::new(RunnerConfig::default());
    runner.register(Arc::new(MatterNearDupHandler::new()));

    let params = JobParams::new(
        serde_json::json!({
            "reset": true,
            "batch_size": 10,
            "threshold": 0.80
        })
        .to_string(),
    );
    let job_id = runner
        .start(&root, "neardup", params)
        .expect("start neardup");
    assert!(runner.wait_until_idle(Duration::from_secs(30)));

    let matter = Matter::open(&root).expect("open");
    let jobs = matter.list_jobs().expect("list");
    assert_eq!(jobs.len(), 1, "Option C: exactly one job row");
    assert_eq!(jobs[0].id, job_id);
    assert_eq!(jobs[0].kind, "neardup");
    assert_eq!(jobs[0].state, JobState::Succeeded);

    let cands = matter.list_neardup_candidates(true).expect("cands");
    assert_eq!(cands.len(), 2);
    let a = matter
        .get_item(
            &cands
                .iter()
                .find(|c| c.path.as_deref() == Some("a.txt"))
                .unwrap()
                .id,
        )
        .unwrap();
    let b = matter
        .get_item(
            &cands
                .iter()
                .find(|c| c.path.as_deref() == Some("b.txt"))
                .unwrap()
                .id,
        )
        .unwrap();
    assert_eq!(a.near_dup_group_id, b.near_dup_group_id);
    assert!(a.near_dup_group_id.is_some());
    let roles = [a.near_dup_role.as_deref(), b.near_dup_role.as_deref()];
    assert!(roles.contains(&Some(item_near_dup_role::PIVOT)));
    assert!(roles.contains(&Some(item_near_dup_role::MEMBER)));

    let cp = matter
        .get_checkpoint(&job_id, "neardup")
        .expect("cp")
        .expect("neardup checkpoint");
    assert!(cp.completed_count >= 2);
}

#[cfg(feature = "cull")]
#[test]
fn cull_via_runner_one_job_row() {
    use matter_core::{item_cull_status, item_dedup_role, item_role, item_status, ItemInput};
    use process_runner::MatterCullHandler;

    let (_tmp, base) = utf8_tempdir();
    let root = make_matter(&base, "m-cull");

    {
        let matter = Matter::open(&root).expect("open");
        matter
            .insert_item(ItemInput {
                status: item_status::EXTRACTED.into(),
                role: Some(item_role::STANDALONE.into()),
                path: Some("unique.eml".into()),
                dedup_role: Some(item_dedup_role::UNIQUE.into()),
                size_bytes: Some(10),
                ..Default::default()
            })
            .expect("u");
        matter
            .insert_item(ItemInput {
                status: item_status::EXTRACTED.into(),
                role: Some(item_role::STANDALONE.into()),
                path: Some("dup.eml".into()),
                dedup_role: Some(item_dedup_role::DUPLICATE.into()),
                size_bytes: Some(10),
                ..Default::default()
            })
            .expect("d");
    }

    let mut runner = ProcessRunner::new(RunnerConfig::default());
    runner.register(Arc::new(MatterCullHandler::new()));

    let params = JobParams::new(
        serde_json::json!({
            "preset_name": "unique_only",
            "reset": true,
            "batch_size": 10
        })
        .to_string(),
    );
    let job_id = runner.start(&root, "cull", params).expect("start cull");
    assert!(runner.wait_until_idle(Duration::from_secs(30)));

    let matter = Matter::open(&root).expect("open");
    let jobs = matter.list_jobs().expect("list");
    assert_eq!(jobs.len(), 1, "Option C: exactly one job row");
    assert_eq!(jobs[0].id, job_id);
    assert_eq!(jobs[0].kind, "cull");
    assert_eq!(jobs[0].state, JobState::Succeeded);

    let cands = matter.list_cull_candidates(true).expect("cands");
    assert_eq!(cands.len(), 2);
    let u = matter
        .get_item(
            &cands
                .iter()
                .find(|c| c.path.as_deref() == Some("unique.eml"))
                .unwrap()
                .id,
        )
        .unwrap();
    let d = matter
        .get_item(
            &cands
                .iter()
                .find(|c| c.path.as_deref() == Some("dup.eml"))
                .unwrap()
                .id,
        )
        .unwrap();
    assert_eq!(u.cull_status.as_deref(), Some(item_cull_status::INCLUDED));
    assert_eq!(d.cull_status.as_deref(), Some(item_cull_status::CULLED));

    let cp = matter
        .get_checkpoint(&job_id, "cull")
        .expect("cp")
        .expect("cull checkpoint");
    assert!(cp.completed_count >= 2);
}

#[cfg(feature = "promote")]
#[test]
fn promote_via_runner_one_job_row() {
    use matter_core::{item_dedup_role, item_role, item_status, ItemInput};
    use process_runner::MatterPromoteHandler;

    let (_tmp, base) = utf8_tempdir();
    let root = make_matter(&base, "m-promote");

    {
        let matter = Matter::open(&root).expect("open");
        matter
            .insert_item(ItemInput {
                status: item_status::EXTRACTED.into(),
                role: Some(item_role::STANDALONE.into()),
                path: Some("unique.eml".into()),
                dedup_role: Some(item_dedup_role::UNIQUE.into()),
                size_bytes: Some(10),
                ..Default::default()
            })
            .expect("u");
        matter
            .insert_item(ItemInput {
                status: item_status::EXTRACTED.into(),
                role: Some(item_role::STANDALONE.into()),
                path: Some("dup.eml".into()),
                dedup_role: Some(item_dedup_role::DUPLICATE.into()),
                size_bytes: Some(10),
                ..Default::default()
            })
            .expect("d");
    }

    let mut runner = ProcessRunner::new(RunnerConfig::default());
    runner.register(Arc::new(MatterPromoteHandler::new()));

    let params = JobParams::new(
        serde_json::json!({
            "policy": "unique_only",
            "expand_families": false,
            "reset": true,
            "batch_size": 10
        })
        .to_string(),
    );
    let job_id = runner
        .start(&root, "promote", params)
        .expect("start promote");
    assert!(runner.wait_until_idle(Duration::from_secs(30)));

    let matter = Matter::open(&root).expect("open");
    let jobs = matter.list_jobs().expect("list");
    assert_eq!(jobs.len(), 1, "Option C: exactly one job row");
    assert_eq!(jobs[0].id, job_id);
    assert_eq!(jobs[0].kind, "promote");
    assert_eq!(jobs[0].state, JobState::Succeeded);

    let cands = matter.list_promote_candidates().expect("cands");
    assert_eq!(cands.len(), 2);
    let u = cands
        .iter()
        .find(|c| c.path.as_deref() == Some("unique.eml"))
        .unwrap();
    let d = cands
        .iter()
        .find(|c| c.path.as_deref() == Some("dup.eml"))
        .unwrap();
    assert_eq!(matter.get_item(&u.id).unwrap().in_review, Some(1));
    assert_ne!(matter.get_item(&d.id).unwrap().in_review, Some(1));

    let cp = matter
        .get_checkpoint(&job_id, "promote")
        .expect("cp")
        .expect("promote checkpoint");
    assert!(cp.completed_count >= 1);

    let sets = matter.list_review_sets().expect("sets");
    assert_eq!(sets.len(), 1);
    assert!(sets[0].is_default);
    assert_eq!(sets[0].item_count, 1);
}

/// Resume must restore full nested `params` from the checkpoint cursor (not just
/// `source_id` / empty object), so non-default dedupe settings survive cancel.
#[test]
fn resume_loads_full_params_from_checkpoint() {
    use std::sync::Mutex;

    struct CaptureParamsHandler {
        seen: Arc<Mutex<Vec<(bool, String)>>>,
    }

    impl JobHandler for CaptureParamsHandler {
        fn kind(&self) -> &'static str {
            "mock_params"
        }

        fn run(&self, ctx: &JobContext<'_>) -> Result<JobOutcome, RunnerError> {
            self.seen
                .lock()
                .expect("lock")
                .push((ctx.is_resume, ctx.params_json.to_string()));

            if !ctx.is_resume {
                let cursor = serde_json::json!({
                    "cursor_index": 1,
                    "completed_count": 1,
                    "unique": 1,
                    "duplicate": 0,
                    "skipped": 0,
                    "mid_logical_conflicts": 0,
                    "phase": "parents",
                    "family_cursor": 0,
                    "params": {
                        "use_message_id": false,
                        "use_logical_hash": true,
                        "family_policy": "parents_only",
                        "reset": false,
                        "batch_size": 7
                    }
                });
                ctx.matter
                    .put_checkpoint(ctx.job_id, "dedupe", &cursor.to_string(), 1)
                    .map_err(RunnerError::from)?;
                ctx.matter
                    .set_job_state(ctx.job_id, JobState::Paused, Some("midway"))
                    .map_err(RunnerError::from)?;
                return Ok(JobOutcome::Paused {
                    message: Some("midway".into()),
                    completed_count: 1,
                });
            }

            ctx.matter
                .set_job_state(ctx.job_id, JobState::Succeeded, None)
                .map_err(RunnerError::from)?;
            Ok(JobOutcome::Succeeded {
                message: Some("resumed".into()),
                completed_count: 2,
            })
        }
    }

    let (_tmp, base) = utf8_tempdir();
    let root = make_matter(&base, "m-resume-params");
    let seen = Arc::new(Mutex::new(Vec::new()));

    let mut runner = ProcessRunner::new(RunnerConfig::default());
    runner.register(Arc::new(CaptureParamsHandler {
        seen: Arc::clone(&seen),
    }));

    let job_id = runner
        .start(
            &root,
            "mock_params",
            JobParams::new(r#"{"use_message_id":true,"batch_size":99}"#),
        )
        .expect("start");
    assert!(runner.wait_until_idle(Duration::from_secs(5)));

    runner.resume(&root, &job_id).expect("resume");
    assert!(runner.wait_until_idle(Duration::from_secs(5)));

    let shots = seen.lock().expect("lock");
    assert_eq!(shots.len(), 2, "start + resume");
    assert!(!shots[0].0, "first run is not resume");
    assert!(shots[1].0, "second run is resume");

    let resumed: serde_json::Value = serde_json::from_str(&shots[1].1).expect("resume params json");
    assert_eq!(
        resumed.get("use_message_id").and_then(|v| v.as_bool()),
        Some(false),
        "resume must restore nested checkpoint params, got {}",
        shots[1].1
    );
    assert_eq!(
        resumed.get("family_policy").and_then(|v| v.as_str()),
        Some("parents_only")
    );
    assert_eq!(resumed.get("batch_size").and_then(|v| v.as_u64()), Some(7));
}

#[cfg(feature = "office")]
#[test]
fn office_extract_handler_via_process_runner() {
    use std::fs;
    use std::path::PathBuf;

    use matter_core::ItemInput;
    use process_runner::MatterOfficeExtractHandler;

    let (_tmp, base) = utf8_tempdir();
    let root = make_matter(&base, "m-office");

    // Load synthetic fixture from workspace fixtures/office.
    let mut fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    fixture.pop(); // crates
    fixture.pop(); // workspace
    fixture.push("fixtures");
    fixture.push("office");
    fixture.push("minimal.docx");
    let data = fs::read(&fixture).expect("fixture");

    {
        let matter = Matter::open(&root).expect("open");
        let native = matter.put_bytes(&data).expect("put");
        matter
            .insert_item(ItemInput {
                path: Some("memo.docx".into()),
                native_sha256: Some(native),
                status: "extracted".into(),
                file_category: Some("attachment".into()),
                ..Default::default()
            })
            .expect("item");
    }

    let mut runner = ProcessRunner::new(RunnerConfig::default());
    runner.register(Arc::new(MatterOfficeExtractHandler::new()));

    let params = JobParams::new(
        serde_json::json!({ "force": false, "batch_size": 10, "formats": ["docx","xlsx","pptx"] })
            .to_string(),
    );
    let job_id = runner
        .start(&root, "office_extract", params)
        .expect("start office_extract");
    assert!(runner.wait_until_idle(Duration::from_secs(30)));

    let matter = Matter::open(&root).expect("open");
    let job = matter.get_job(&job_id).expect("job");
    assert_eq!(
        job.state,
        JobState::Succeeded,
        "err={:?}",
        job.error_summary
    );
    assert_eq!(job.kind, "office_extract");

    let cands = matter
        .list_office_candidates(0, 10, false)
        .expect("candidates");
    assert_eq!(cands.len(), 1);
    let item = matter.get_item(&cands[0].id).expect("item");
    assert!(item.text_sha256.is_some(), "handler must write text_sha256");
    assert_eq!(item.office_extract_status.as_deref(), Some("ok"));
}

#[cfg(feature = "pdf")]
#[test]
fn pdf_extract_handler_via_process_runner() {
    use std::fs;
    use std::path::PathBuf;

    use matter_core::ItemInput;
    use process_runner::MatterPdfExtractHandler;

    let (_tmp, base) = utf8_tempdir();
    let root = make_matter(&base, "m-pdf");

    let mut fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    fixture.pop();
    fixture.pop();
    fixture.push("fixtures");
    fixture.push("pdf");
    fixture.push("minimal.pdf");
    let data = fs::read(&fixture).expect("fixture");

    {
        let matter = Matter::open(&root).expect("open");
        let native = matter.put_bytes(&data).expect("put");
        matter
            .insert_item(ItemInput {
                path: Some("memo.pdf".into()),
                native_sha256: Some(native),
                status: "extracted".into(),
                file_category: Some("attachment".into()),
                ..Default::default()
            })
            .expect("item");
    }

    let mut runner = ProcessRunner::new(RunnerConfig::default());
    runner.register(Arc::new(MatterPdfExtractHandler::new()));

    let params =
        JobParams::new(serde_json::json!({ "force": false, "batch_size": 10 }).to_string());
    let job_id = runner
        .start(&root, "pdf_extract", params)
        .expect("start pdf_extract");
    assert!(runner.wait_until_idle(Duration::from_secs(30)));

    let matter = Matter::open(&root).expect("open");
    let job = matter.get_job(&job_id).expect("job");
    assert_eq!(
        job.state,
        JobState::Succeeded,
        "err={:?}",
        job.error_summary
    );
    assert_eq!(job.kind, "pdf_extract");

    let cands = matter
        .list_pdf_candidates(0, 10, false)
        .expect("candidates");
    assert_eq!(cands.len(), 1);
    let item = matter.get_item(&cands[0].id).expect("item");
    assert!(item.text_sha256.is_some(), "handler must write text_sha256");
    assert_eq!(item.pdf_extract_status.as_deref(), Some("ok"));
    assert_eq!(item.pdf_needs_ocr, 0);
}

#[cfg(feature = "ocr")]
#[test]
fn ocr_handler_rejects_production_mock_engine() {
    use matter_core::ItemInput;
    use ocr_plugin::minimal_png_bytes;
    use process_runner::MatterOcrHandler;

    let (_tmp, base) = utf8_tempdir();
    let root = make_matter(&base, "m-ocr-mock-reject");

    {
        let matter = Matter::open(&root).expect("open");
        let native = matter.put_bytes(&minimal_png_bytes()).expect("put");
        matter
            .insert_item(ItemInput {
                path: Some("scan.png".into()),
                native_sha256: Some(native),
                status: "extracted".into(),
                mime_type: Some("image/png".into()),
                file_category: Some("image".into()),
                ..Default::default()
            })
            .expect("item");
    }

    let mut runner = ProcessRunner::new(RunnerConfig::default());
    runner.register(Arc::new(MatterOcrHandler::new()));

    // Production path must reject engine=mock (no fabricated OCR via job JSON).
    let params = JobParams::new(
        serde_json::json!({
            "force": false,
            "batch_size": 10,
            "lang": "eng",
            "enabled": true,
            "engine": "mock"
        })
        .to_string(),
    );
    let job_id = runner.start(&root, "ocr", params).expect("start ocr");
    assert!(runner.wait_until_idle(Duration::from_secs(30)));

    let matter = Matter::open(&root).expect("open");
    let job = matter.get_job(&job_id).expect("job");
    assert_eq!(
        job.state,
        JobState::Failed,
        "mock must fail: {:?}",
        job.error_summary
    );
    assert!(
        job.error_summary
            .as_deref()
            .unwrap_or("")
            .to_ascii_lowercase()
            .contains("mock"),
        "err={:?}",
        job.error_summary
    );

    let item = matter
        .list_ocr_candidates(0, 10, true)
        .expect("candidates")
        .into_iter()
        .next()
        .expect("item");
    let after = matter.get_item(&item.id).expect("get");
    assert!(after.ocr_status.is_none(), "no item mutation");
    assert!(after.text_sha256.is_none());
}

#[cfg(feature = "ocr")]
#[test]
fn ocr_plugin_mock_via_injection_works() {
    // Mock success is only via run_ocr_with_engine (tests), not production handler.
    use matter_core::ItemInput;
    use ocr_plugin::{
        minimal_png_bytes, run_ocr_with_engine, MockOcrEngine, OcrParams, JOB_KIND_OCR,
    };

    let (_tmp, base) = utf8_tempdir();
    let root = make_matter(&base, "m-ocr-inject");
    let matter = Matter::open(&root).expect("open");
    let native = matter.put_bytes(&minimal_png_bytes()).expect("put");
    let item = matter
        .insert_item(ItemInput {
            path: Some("scan.png".into()),
            native_sha256: Some(native),
            status: "extracted".into(),
            mime_type: Some("image/png".into()),
            file_category: Some("image".into()),
            ..Default::default()
        })
        .expect("item");
    let job = matter.create_job(JOB_KIND_OCR).expect("job");
    let engine = MockOcrEngine::new("INJECTED_MOCK_OK");
    let params = OcrParams {
        enabled: true,
        engine: "tesseract".into(), // injection ignores engine string for selection
        ..OcrParams::default()
    };
    let outcome =
        run_ocr_with_engine(&matter, &job.id, &params, &engine, None, |_| {}).expect("run");
    assert!(matches!(outcome, ocr_plugin::OcrOutcome::Succeeded(_)));
    let after = matter.get_item(&item.id).expect("get");
    assert_eq!(after.ocr_status.as_deref(), Some("ok"));
    assert!(after.text_sha256.is_some());
}

#[cfg(feature = "ocr")]
#[test]
fn ocr_handler_disabled_fails_closed() {
    use matter_core::ItemInput;
    use ocr_plugin::minimal_png_bytes;
    use process_runner::MatterOcrHandler;

    let (_tmp, base) = utf8_tempdir();
    let root = make_matter(&base, "m-ocr-off");

    {
        let matter = Matter::open(&root).expect("open");
        let native = matter.put_bytes(&minimal_png_bytes()).expect("put");
        matter
            .insert_item(ItemInput {
                path: Some("scan.png".into()),
                native_sha256: Some(native),
                status: "extracted".into(),
                mime_type: Some("image/png".into()),
                ..Default::default()
            })
            .expect("item");
    }

    let mut runner = ProcessRunner::new(RunnerConfig::default());
    runner.register(Arc::new(MatterOcrHandler::new()));

    let params = JobParams::new(
        serde_json::json!({
            "enabled": false,
            "engine": "mock"
        })
        .to_string(),
    );
    let job_id = runner.start(&root, "ocr", params).expect("start ocr");
    assert!(runner.wait_until_idle(Duration::from_secs(30)));

    let matter = Matter::open(&root).expect("open");
    let job = matter.get_job(&job_id).expect("job");
    assert_eq!(job.state, JobState::Failed, "expected fail closed");
    let item = matter
        .list_ocr_candidates(0, 10, false)
        .expect("c")
        .into_iter()
        .next()
        .expect("cand");
    let full = matter.get_item(&item.id).expect("item");
    assert!(full.ocr_status.is_none());
    assert!(full.text_sha256.is_none());
}

/// Thin classify handler smoke: attachment → pdf via path extension.
#[cfg(feature = "classify")]
#[test]
fn classify_handler_via_process_runner() {
    use matter_core::{item_role, item_status, ItemInput};
    use process_runner::MatterClassifyHandler;

    let (_tmp, base) = utf8_tempdir();
    let root = make_matter(&base, "m-classify");

    let item_id = {
        let matter = Matter::open(&root).expect("open");
        matter
            .insert_item(ItemInput {
                path: Some("report.pdf".into()),
                status: item_status::EXTRACTED.into(),
                file_category: Some("attachment".into()),
                role: Some(item_role::ATTACHMENT.into()),
                size_bytes: Some(10),
                ..Default::default()
            })
            .expect("item")
            .id
    };

    let mut runner = ProcessRunner::new(RunnerConfig::default());
    runner.register(Arc::new(MatterClassifyHandler::new()));

    let job_id = runner
        .start(&root, "classify", JobParams::empty())
        .expect("start classify");
    assert!(runner.wait_until_idle(Duration::from_secs(30)));

    let matter = Matter::open(&root).expect("open");
    let job = matter.get_job(&job_id).expect("job");
    assert_eq!(
        job.state,
        JobState::Succeeded,
        "err={:?}",
        job.error_summary
    );
    assert_eq!(job.kind, "classify");

    let item = matter.get_item(&item_id).expect("item");
    assert_eq!(item.file_category.as_deref(), Some("pdf"));
    assert_eq!(item.category_taxonomy.as_deref(), Some("taxonomy_v1"));
    assert_eq!(item.role.as_deref(), Some(item_role::ATTACHMENT));
}

/// Thin entity_scan handler smoke: email + Luhn card → masked hits.
#[cfg(feature = "entity")]
#[test]
fn entity_scan_handler_via_process_runner() {
    use matter_core::{item_status, ItemInput};
    use process_runner::MatterEntityScanHandler;

    let (_tmp, base) = utf8_tempdir();
    let root = make_matter(&base, "m-entity");

    let item_id = {
        let matter = Matter::open(&root).expect("open");
        let text = matter
            .put_bytes(b"Contact bob@competitor.com card 4111111111111111")
            .expect("put");
        matter
            .insert_item(ItemInput {
                path: Some("msg.txt".into()),
                status: item_status::EXTRACTED.into(),
                text_sha256: Some(text),
                subject: Some("Invoice".into()),
                ..Default::default()
            })
            .expect("item")
            .id
    };

    let mut runner = ProcessRunner::new(RunnerConfig::default());
    runner.register(Arc::new(MatterEntityScanHandler::new()));

    let job_id = runner
        .start(&root, "entity_scan", JobParams::empty())
        .expect("start entity_scan");
    assert!(runner.wait_until_idle(Duration::from_secs(30)));

    let matter = Matter::open(&root).expect("open");
    let job = matter.get_job(&job_id).expect("job");
    assert_eq!(
        job.state,
        JobState::Succeeded,
        "err={:?}",
        job.error_summary
    );
    assert_eq!(job.kind, "entity_scan");

    let hits = matter.list_entity_hits(&item_id).expect("hits");
    assert!(!hits.is_empty());
    assert!(hits.iter().any(|h| h.entity_type == "email"));
    assert!(hits.iter().any(|h| h.entity_type == "credit_card"));
    for h in &hits {
        assert!(!h.masked_value.contains("4111111111111111"));
    }
}

/// Thin sentiment handler smoke: clear positive → positive polarity.
#[cfg(feature = "sentiment")]
#[test]
fn sentiment_handler_via_process_runner() {
    use matter_core::{item_status, ItemInput};
    use process_runner::MatterSentimentHandler;

    let (_tmp, base) = utf8_tempdir();
    let root = make_matter(&base, "m-sentiment");

    let item_id = {
        let matter = Matter::open(&root).expect("open");
        let text = matter
            .put_bytes(b"This is wonderful amazing excellent fantastic great news!!!")
            .expect("put");
        matter
            .insert_item(ItemInput {
                path: Some("msg.txt".into()),
                status: item_status::EXTRACTED.into(),
                text_sha256: Some(text),
                subject: Some("Good news".into()),
                ..Default::default()
            })
            .expect("item")
            .id
    };

    let mut runner = ProcessRunner::new(RunnerConfig::default());
    runner.register(Arc::new(MatterSentimentHandler::new()));

    let job_id = runner
        .start(&root, "sentiment", JobParams::empty())
        .expect("start sentiment");
    assert!(runner.wait_until_idle(Duration::from_secs(30)));

    let matter = Matter::open(&root).expect("open");
    let job = matter.get_job(&job_id).expect("job");
    assert_eq!(
        job.state,
        JobState::Succeeded,
        "err={:?}",
        job.error_summary
    );
    assert_eq!(job.kind, "sentiment");

    let item = matter.get_item(&item_id).expect("item");
    assert_eq!(item.sentiment_polarity.as_deref(), Some("positive"));
    assert!(item.sentiment_compound.is_some());
    assert_eq!(item.sentiment_method.as_deref(), Some("vader_lexicon_v1"));
}

/// Thin semantic_index handler smoke: mock embed + meta enabled.
#[cfg(feature = "semantic")]
#[test]
fn semantic_index_handler_via_process_runner() {
    use matter_core::{item_status, ItemInput};
    use process_runner::MatterSemanticIndexHandler;

    let (_tmp, base) = utf8_tempdir();
    let root = make_matter(&base, "m-semantic");

    let item_id = {
        let matter = Matter::open(&root).expect("open");
        let text = matter
            .put_bytes(b"fraud investigation bribery scheme confidential documents")
            .expect("put");
        matter
            .insert_item(ItemInput {
                path: Some("msg.txt".into()),
                status: item_status::EXTRACTED.into(),
                text_sha256: Some(text),
                subject: Some("Investigation".into()),
                ..Default::default()
            })
            .expect("item")
            .id
    };

    let mut runner = ProcessRunner::new(RunnerConfig::default());
    runner.register(Arc::new(MatterSemanticIndexHandler::new()));

    let job_id = runner
        .start(&root, "semantic_index", JobParams::empty())
        .expect("start semantic_index");
    assert!(runner.wait_until_idle(Duration::from_secs(30)));

    let matter = Matter::open(&root).expect("open");
    let job = matter.get_job(&job_id).expect("job");
    assert_eq!(
        job.state,
        JobState::Succeeded,
        "err={:?}",
        job.error_summary
    );
    assert_eq!(job.kind, "semantic_index");

    let item = matter.get_item(&item_id).expect("item");
    assert!(item.semantic_embedded_text_sha256.is_some());
    assert!(item.semantic_chunk_count.unwrap_or(0) >= 1);

    let meta = matter.get_semantic_meta().expect("meta");
    assert!(meta.semantic_enabled);
    assert_eq!(meta.semantic_model_id.as_deref(), Some("mock:hash_v1"));
}

/// people_graph via ProcessRunner + register_default_handlers: job succeeds, people/edges, audit.
#[cfg(feature = "people")]
#[test]
fn people_graph_handler_via_process_runner() {
    use matter_core::{item_status, ItemInput};
    use process_runner::register_default_handlers;

    let (_tmp, base) = utf8_tempdir();
    let root = make_matter(&base, "m-people");

    {
        let matter = Matter::open(&root).expect("open");
        matter
            .insert_item(ItemInput {
                path: Some("msg.eml".into()),
                status: item_status::EXTRACTED.into(),
                from_addr: Some("alice@example.com".into()),
                to_addrs_json: Some(r#"["bob@example.com"]"#.into()),
                cc_addrs_json: Some(r#"["carol@example.com"]"#.into()),
                sent_at: Some("2024-06-01T12:00:00Z".into()),
                ..Default::default()
            })
            .expect("item");
    }

    let mut runner = ProcessRunner::new(RunnerConfig::default());
    register_default_handlers(&mut runner);

    let params = JobParams::new(
        serde_json::json!({
            "scope": "all",
            "include_entity_emails": false,
            "grain": "day",
            "reset": true,
            "batch_size": 200,
            "max_recipients_per_item": 200
        })
        .to_string(),
    );
    let job_id = runner
        .start(&root, "people_graph", params)
        .expect("start people_graph");
    assert!(runner.wait_until_idle(Duration::from_secs(30)));

    let matter = Matter::open(&root).expect("open");
    let job = matter.get_job(&job_id).expect("job");
    assert_eq!(
        job.state,
        JobState::Succeeded,
        "err={:?}",
        job.error_summary
    );
    assert_eq!(job.kind, "people_graph");

    let people = matter.list_people(50).expect("people");
    assert!(
        people.len() >= 3,
        "expected alice/bob/carol people, got {}",
        people.len()
    );
    let edges = matter.list_people_edges(50).expect("edges");
    assert!(
        !edges.is_empty(),
        "expected at least one visible edge (alice→bob and/or alice→carol)"
    );
    assert!(edges.iter().all(|e| e.visible_count > 0));

    let status = matter.people_graph_status().expect("status");
    assert!(status.is_complete, "graph should be complete");
    assert!(status.people_count >= 3);
    assert!(status.edge_count >= 1);

    let mut stmt = matter
        .connection()
        .prepare(
            "SELECT action FROM audit_events \
             WHERE action IN ('people_graph.start', 'people_graph.complete') \
             ORDER BY seq ASC",
        )
        .expect("prep audit");
    let actions: Vec<String> = stmt
        .query_map([], |row| row.get(0))
        .expect("q")
        .map(|r| r.expect("row"))
        .collect();
    assert!(
        actions.iter().any(|a| a == "people_graph.start"),
        "missing people_graph.start audit"
    );
    assert!(
        actions.iter().any(|a| a == "people_graph.complete"),
        "missing people_graph.complete audit"
    );
}

/// concept_cluster via ProcessRunner: multi-topic texts → clusters + audit.
#[cfg(feature = "cluster")]
#[test]
fn concept_cluster_handler_via_process_runner() {
    use matter_core::{item_status, ItemInput};
    use process_runner::register_default_handlers;

    let (_tmp, base) = utf8_tempdir();
    let root = make_matter(&base, "m-cluster");

    {
        let matter = Matter::open(&root).expect("open");
        for i in 0..4 {
            let body =
                format!("Invoice payment vendor remittance accounts payable overdue wire {i}");
            let dig = matter.put_bytes(body.as_bytes()).expect("cas");
            matter
                .insert_item(ItemInput {
                    path: Some(format!("inv{i}.txt")),
                    status: item_status::EXTRACTED.into(),
                    text_sha256: Some(dig),
                    ..Default::default()
                })
                .expect("item");
        }
        for i in 0..4 {
            let body =
                format!("Patient clinical dosage pharmaceutical trial laboratory biomarkers {i}");
            let dig = matter.put_bytes(body.as_bytes()).expect("cas");
            matter
                .insert_item(ItemInput {
                    path: Some(format!("clin{i}.txt")),
                    status: item_status::EXTRACTED.into(),
                    text_sha256: Some(dig),
                    ..Default::default()
                })
                .expect("item");
        }
    }

    let mut runner = ProcessRunner::new(RunnerConfig::default());
    register_default_handlers(&mut runner);

    let params = JobParams::new(
        serde_json::json!({
            "set_name": "default",
            "k": 2,
            "seed": 42,
            "min_df": 1,
            "max_df_ratio": 1.0,
            "max_vocab": 5000,
            "label_terms": 5,
            "scope": "all",
            "reset": true,
            "batch_size": 100
        })
        .to_string(),
    );
    let job_id = runner
        .start(&root, "concept_cluster", params)
        .expect("start concept_cluster");
    assert!(runner.wait_until_idle(Duration::from_secs(30)));

    let matter = Matter::open(&root).expect("open");
    let job = matter.get_job(&job_id).expect("job");
    assert_eq!(
        job.state,
        JobState::Succeeded,
        "err={:?}",
        job.error_summary
    );
    assert_eq!(job.kind, "concept_cluster");

    let status = matter.concept_cluster_status("default").expect("status");
    assert!(status.is_complete, "cluster set should be complete");
    assert!(status.cluster_count >= 1);
    assert!(status.item_count >= 1);

    let set_id = status.set_id.expect("set_id");
    let clusters = matter.list_concept_clusters(&set_id).expect("list");
    assert_eq!(clusters.len() as i64, status.cluster_count);
    assert!(clusters.iter().all(|c| c.item_count > 0));

    let mut stmt = matter
        .connection()
        .prepare(
            "SELECT action FROM audit_events \
             WHERE action IN ('concept_cluster.start', 'concept_cluster.complete') \
             ORDER BY seq ASC",
        )
        .expect("prep audit");
    let actions: Vec<String> = stmt
        .query_map([], |row| row.get(0))
        .expect("q")
        .map(|r| r.expect("row"))
        .collect();
    assert!(
        actions.iter().any(|a| a == "concept_cluster.start"),
        "missing concept_cluster.start audit"
    );
    assert!(
        actions.iter().any(|a| a == "concept_cluster.complete"),
        "missing concept_cluster.complete audit"
    );
}

/// Thin production QC handler smoke (`kind = "qc"`).
#[cfg(feature = "qc")]
#[test]
fn qc_handler_via_process_runner() {
    use matter_core::{item_role, item_status, ItemInput};
    use process_runner::MatterQcHandler;

    let (_tmp, base) = utf8_tempdir();
    let root = make_matter(&base, "m-qc");

    {
        let matter = Matter::open(&root).expect("open");
        let native = matter.put_bytes(b"native").expect("put");
        let text = matter.put_bytes(b"plain text body").expect("put text");
        matter
            .insert_item(ItemInput {
                path: Some("memo.pdf".into()),
                native_sha256: Some(native),
                text_sha256: Some(text),
                status: item_status::EXTRACTED.into(),
                file_category: Some("document".into()),
                role: Some(item_role::STANDALONE.into()),
                in_review: Some(1),
                size_bytes: Some(6),
                ..Default::default()
            })
            .expect("item");
    }

    let mut runner = ProcessRunner::new(RunnerConfig::default());
    runner.register(Arc::new(MatterQcHandler::new()));

    let params = JobParams::new(
        serde_json::json!({
            "scope": "review_corpus",
            "expand_family_for_scan": false,
            "profile": "default_production_qc_v1",
            "rules": []
        })
        .to_string(),
    );
    let job_id = runner.start(&root, "qc", params).expect("start qc");
    assert!(runner.wait_until_idle(Duration::from_secs(30)));

    let matter = Matter::open(&root).expect("open");
    let job = matter.get_job(&job_id).expect("job");
    assert_eq!(
        job.state,
        JobState::Succeeded,
        "err={:?}",
        job.error_summary
    );
    assert_eq!(job.kind, "qc");
    let run = matter.load_latest_qc_run().expect("load").expect("qc_run");
    assert!(run.passed, "clean doc should pass QC");
    assert_eq!(run.candidate_count, 1);
}

/// Resume must restore non-default QC params from the `qc` checkpoint stage
/// (not fall back to `{}` / defaults which would fail params_match and restart).
#[test]
fn qc_resume_restores_checkpoint_params() {
    use matter_core::{item_role, item_status, ItemInput, JobState};
    use process_runner::MatterQcHandler;

    let (_tmp, base) = utf8_tempdir();
    let root = make_matter(&base, "m-qc-resume");

    let params_json = serde_json::json!({
        "scope": "review_corpus",
        "expand_family_for_scan": true,
        "profile": "default_production_qc_v1",
        "rules": [],
        "item_ids": [],
        "report_dir": null
    });

    let job_id = {
        let matter = Matter::open(&root).expect("open");
        let native = matter.put_bytes(b"native").expect("put");
        let text = matter.put_bytes(b"plain text body").expect("put text");
        matter
            .insert_item(ItemInput {
                path: Some("memo.pdf".into()),
                native_sha256: Some(native),
                text_sha256: Some(text),
                status: item_status::EXTRACTED.into(),
                file_category: Some("document".into()),
                role: Some(item_role::STANDALONE.into()),
                in_review: Some(1),
                size_bytes: Some(6),
                ..Default::default()
            })
            .expect("item");

        let job = matter.create_job("qc").expect("create job");
        matter
            .set_job_state(&job.id, JobState::Running, None)
            .expect("running");
        matter
            .set_job_state(&job.id, JobState::Paused, Some("cancelled"))
            .expect("pause");
        // Partial checkpoint mid-eval with non-default expand_family_for_scan.
        let cursor = serde_json::json!({
            "phase": "eval",
            "cursor_index": 0,
            "completed_count": 0,
            "candidate_count": 1,
            "params": params_json,
            "ordered_ids": [],
            "findings": [],
            "error_count": 0,
            "warn_count": 0,
            "withheld_count": 0,
            "selection_fingerprint": "",
            "profile": "default_production_qc_v1",
            "scope": "review_corpus"
        });
        matter
            .put_checkpoint(&job.id, "qc", &cursor.to_string(), 0)
            .expect("checkpoint");
        job.id
    };

    let mut runner = ProcessRunner::new(RunnerConfig::default());
    runner.register(Arc::new(MatterQcHandler::new()));
    runner.resume(&root, &job_id).expect("resume qc");
    assert!(runner.wait_until_idle(Duration::from_secs(30)));

    let matter = Matter::open(&root).expect("open");
    let job = matter.get_job(&job_id).expect("job");
    assert_eq!(
        job.state,
        JobState::Succeeded,
        "err={:?}",
        job.error_summary
    );
    let run = matter.load_latest_qc_run().expect("load").expect("qc_run");
    assert!(run.passed);
    assert_eq!(run.candidate_count, 1);
}

// ---------------------------------------------------------------------------
// profile_run (track 0043)
// ---------------------------------------------------------------------------

/// Corrupt parent checkpoint must fail closed on resume (no silent empty restart).
#[test]
fn profile_run_corrupt_checkpoint_fails_closed() {
    use matter_core::{ProcessingProfileInput, JOB_KIND_PROFILE_RUN};
    use process_runner::MatterProfileRunHandler;

    let (_tmp, base) = utf8_tempdir();
    let root = make_matter(&base, "m-profile-corrupt-cp");

    let job_id = {
        let matter = Matter::open(&root).expect("open");
        let body = r#"{
            "version": 1,
            "stages": {
                "classify": { "enabled": true, "params": { "force": false } }
            }
        }"#;
        matter
            .upsert_processing_profile(ProcessingProfileInput {
                id: None,
                name: "classify_only_corrupt".into(),
                description: None,
                body_json: body.into(),
                created_by: None,
            })
            .expect("upsert");
        let job = matter.create_job(JOB_KIND_PROFILE_RUN).expect("create job");
        // Valid JSON so resume can restore params, but invalid ProfileRunCursor
        // shape (stages must be an array of objects).
        let corrupt = serde_json::json!({
            "params": { "profile_name": "classify_only_corrupt" },
            "stages": "not-an-array"
        });
        matter
            .put_checkpoint(&job.id, "profile_run", &corrupt.to_string(), 0)
            .expect("put corrupt checkpoint");
        // Valid transition path for resume: Pending → Running → Paused.
        matter
            .set_job_state(&job.id, JobState::Running, None)
            .expect("running");
        matter
            .set_job_state(&job.id, JobState::Paused, Some("test-pause"))
            .expect("pause");
        job.id
    };

    let mut runner = ProcessRunner::new(RunnerConfig::default());
    runner.register(Arc::new(MatterProfileRunHandler::with_default_handlers()));
    runner.resume(&root, &job_id).expect("resume accepted");
    assert!(
        runner.wait_until_idle(Duration::from_secs(15)),
        "resume did not finish"
    );

    let matter = Matter::open(&root).expect("open");
    let job = matter.get_job(&job_id).expect("job");
    assert_eq!(
        job.state,
        JobState::Failed,
        "corrupt checkpoint must fail closed; err={:?}",
        job.error_summary
    );
    let err = job.error_summary.unwrap_or_default();
    assert!(
        err.contains("corrupt profile_run checkpoint") || err.contains("corrupt"),
        "error should mention corrupt checkpoint, got: {err}"
    );
    // No classify child should have been created (no silent restart).
    let classify: Vec<_> = matter
        .list_jobs()
        .expect("list")
        .into_iter()
        .filter(|j| j.kind == "classify")
        .collect();
    assert!(
        classify.is_empty(),
        "corrupt checkpoint must not spawn stage children"
    );
}

/// Second classify-only profile_run is idempotent when items are already classified.
#[cfg(feature = "classify")]
#[test]
fn profile_run_classify_idempotent_second_run() {
    use matter_core::{
        item_role, item_status, ApplyClassificationInput, ItemInput, ProcessingProfileInput,
        JOB_KIND_PROFILE_RUN,
    };
    use process_runner::MatterProfileRunHandler;

    let (_tmp, base) = utf8_tempdir();
    let root = make_matter(&base, "m-profile-classify-idem");

    let item_id = {
        let matter = Matter::open(&root).expect("open");
        let body = r#"{
            "version": 1,
            "stages": {
                "classify": {
                    "enabled": true,
                    "params": {
                        "force": false,
                        "batch_size": 100,
                        "use_magic": true,
                        "in_review_only": false,
                        "respect_extractor_refine": true
                    }
                }
            }
        }"#;
        matter
            .upsert_processing_profile(ProcessingProfileInput {
                id: None,
                name: "classify_only_idem".into(),
                description: None,
                body_json: body.into(),
                created_by: None,
            })
            .expect("upsert");
        let item = matter
            .insert_item(ItemInput {
                path: Some("memo.txt".into()),
                status: item_status::EXTRACTED.into(),
                file_category: Some("document".into()),
                role: Some(item_role::STANDALONE.into()),
                size_bytes: Some(4),
                ..Default::default()
            })
            .expect("item");
        // Seed decisive taxonomy_v1 so force:false classify skips the row.
        matter
            .apply_classification(ApplyClassificationInput {
                item_id: item.id.clone(),
                force: true,
                category: "document".into(),
                method: "extension".into(),
                taxonomy: "taxonomy_v1".into(),
                mime_type: None,
                status: Some("ok".into()),
                error: None,
            })
            .expect("seed classify");
        item.id
    };

    let mut runner = ProcessRunner::new(RunnerConfig::default());
    runner.register(Arc::new(MatterProfileRunHandler::with_default_handlers()));

    let params = JobParams::new(r#"{"profile_name":"classify_only_idem"}"#);
    let parent1 = runner
        .start(&root, JOB_KIND_PROFILE_RUN, params.clone())
        .expect("start1");
    assert!(runner.wait_until_idle(Duration::from_secs(30)));

    let (method_before, cat_before, categorized_at_before) = {
        let matter = Matter::open(&root).expect("open");
        let p1 = matter.get_job(&parent1).expect("p1");
        assert_eq!(
            p1.state,
            JobState::Succeeded,
            "first run err={:?}",
            p1.error_summary
        );
        let before = matter.get_item(&item_id).expect("item before");
        let method_before = before.category_method.clone();
        let cat_before = before.file_category.clone();
        let categorized_at_before = before.categorized_at.clone();
        assert!(
            categorized_at_before.is_some(),
            "seeded classification should set categorized_at"
        );
        // Drop exclusive write lock before second runner start.
        (method_before, cat_before, categorized_at_before)
    };

    let parent2 = runner
        .start(&root, JOB_KIND_PROFILE_RUN, params)
        .expect("start2");
    assert!(runner.wait_until_idle(Duration::from_secs(30)));

    let matter = Matter::open(&root).expect("reopen");
    let p2 = matter.get_job(&parent2).expect("p2");
    assert_eq!(
        p2.state,
        JobState::Succeeded,
        "second run err={:?}",
        p2.error_summary
    );

    let after = matter.get_item(&item_id).expect("item after");
    assert_eq!(after.file_category, cat_before, "category unchanged");
    assert_eq!(after.category_method, method_before, "method unchanged");
    assert_eq!(
        after.categorized_at, categorized_at_before,
        "categorized_at must not change when force:false skip applies"
    );

    let classify_jobs: Vec<_> = matter
        .list_jobs()
        .expect("list")
        .into_iter()
        .filter(|j| j.kind == "classify")
        .collect();
    assert_eq!(classify_jobs.len(), 2, "one classify child per profile_run");
    for j in &classify_jobs {
        assert_eq!(j.state, JobState::Succeeded, "child {:?}", j.id);
    }
    // Second classify child: force:false filters already-done rows at SQL level
    // → classified_count must be 0 on classify.complete audit (no re-write).
    let second = classify_jobs
        .iter()
        .max_by_key(|j| j.created_at.as_str())
        .expect("second");
    let entity = format!("job:{}", second.id);
    let params_json: String = matter
        .connection()
        .query_row(
            "SELECT params_json FROM audit_events \
             WHERE action = 'classify.complete' AND entity = ?1 \
             ORDER BY seq DESC LIMIT 1",
            [&entity],
            |row| row.get(0),
        )
        .expect("classify.complete audit for second child");
    let audit: serde_json::Value = serde_json::from_str(&params_json).expect("json");
    let classified = audit
        .get("classified_count")
        .and_then(|v| v.as_u64())
        .unwrap_or(u64::MAX);
    assert_eq!(
        classified, 0,
        "second classify must not re-classify seeded item; audit={audit}"
    );
}

#[test]
fn profile_run_creates_child_job_rows() {
    use matter_core::{ProcessingProfileInput, JOB_KIND_PROFILE_RUN};
    use process_runner::MatterProfileRunHandler;

    let (_tmp, base) = utf8_tempdir();
    let root = make_matter(&base, "m-profile");

    // Custom profile: only classify enabled (fast, no external tools).
    {
        let matter = Matter::open(&root).expect("open");
        let body = r#"{
            "version": 1,
            "stages": {
                "classify": { "enabled": true, "params": { "force": false, "batch_size": 100, "use_magic": true, "in_review_only": false, "respect_extractor_refine": true } }
            }
        }"#;
        matter
            .upsert_processing_profile(ProcessingProfileInput {
                id: None,
                name: "classify_only".into(),
                description: None,
                body_json: body.into(),
                created_by: None,
            })
            .expect("upsert profile");
    }

    let mut runner = ProcessRunner::new(RunnerConfig::default());
    runner.register(Arc::new(MatterProfileRunHandler::with_default_handlers()));

    let params = JobParams::new(r#"{"profile_name":"classify_only"}"#);
    let parent_id = runner
        .start(&root, JOB_KIND_PROFILE_RUN, params)
        .expect("start profile_run");
    assert!(
        runner.wait_until_idle(Duration::from_secs(30)),
        "profile_run did not finish"
    );

    let matter = Matter::open(&root).expect("open");
    let parent = matter.get_job(&parent_id).expect("parent");
    assert_eq!(
        parent.state,
        JobState::Succeeded,
        "parent err={:?}",
        parent.error_summary
    );
    assert_eq!(parent.kind, JOB_KIND_PROFILE_RUN);

    let jobs = matter.list_jobs().expect("list");
    assert!(
        jobs.len() >= 2,
        "expected parent + at least one child, got {}",
        jobs.len()
    );
    let classify_children: Vec<_> = jobs.iter().filter(|j| j.kind == "classify").collect();
    assert_eq!(classify_children.len(), 1, "exactly one classify child job");
    assert_eq!(classify_children[0].state, JobState::Succeeded);

    // Parent checkpoint lists stage outcomes.
    let cp = matter
        .get_checkpoint(&parent_id, "profile_run")
        .expect("cp")
        .expect("checkpoint present");
    let cursor: serde_json::Value = serde_json::from_str(&cp.cursor_json).expect("json");
    let stages = cursor["stages"].as_array().expect("stages array");
    assert!(!stages.is_empty());
    assert_eq!(stages[0]["stage"], "classify");
    assert_eq!(stages[0]["status"], "succeeded");
    assert!(!stages[0]["job_id"].as_str().unwrap_or("").is_empty());
}

#[test]
fn profile_run_extract_only_multi_stage_children() {
    use matter_core::JOB_KIND_PROFILE_RUN;
    use process_runner::MatterProfileRunHandler;

    let (_tmp, base) = utf8_tempdir();
    let root = make_matter(&base, "m-profile-extract");

    let mut runner = ProcessRunner::new(RunnerConfig::default());
    runner.register(Arc::new(MatterProfileRunHandler::with_default_handlers()));

    let params = JobParams::new(r#"{"profile_id":"builtin:extract_only"}"#);
    let parent_id = runner
        .start(&root, JOB_KIND_PROFILE_RUN, params)
        .expect("start");
    assert!(runner.wait_until_idle(Duration::from_secs(60)));

    {
        let matter = Matter::open(&root).expect("open");
        let parent = matter.get_job(&parent_id).expect("parent");
        assert_eq!(
            parent.state,
            JobState::Succeeded,
            "err={:?}",
            parent.error_summary
        );

        let jobs = matter.list_jobs().expect("list");
        let kinds: std::collections::BTreeSet<_> = jobs.iter().map(|j| j.kind.as_str()).collect();
        assert!(kinds.contains(JOB_KIND_PROFILE_RUN));
        assert!(kinds.contains("classify"));
        assert!(kinds.contains("office_extract"));
        assert!(kinds.contains("pdf_extract"));
        assert!(kinds.contains("ics_extract"));
        // Distinct child kinds (not only parent).
        let child_kinds: std::collections::BTreeSet<_> = jobs
            .iter()
            .filter(|j| j.kind != JOB_KIND_PROFILE_RUN)
            .map(|j| j.kind.as_str())
            .collect();
        assert!(
            child_kinds.len() >= 2,
            "expected multiple distinct child kinds, got {child_kinds:?}"
        );
        // Drop exclusive write lock before second runner start.
    }

    // Second run: creates new child jobs; classify with force:false skips already-done items
    // (empty matter → completed quickly with skip semantics intact).
    let parent2 = runner
        .start(
            &root,
            JOB_KIND_PROFILE_RUN,
            JobParams::new(r#"{"profile_name":"extract_only"}"#),
        )
        .expect("second run");
    assert!(runner.wait_until_idle(Duration::from_secs(60)));
    // Re-open for fresh connection after runner closed.
    let matter = Matter::open(&root).expect("reopen");
    let parent2_job = matter.get_job(&parent2).expect("p2");
    assert_eq!(
        parent2_job.state,
        JobState::Succeeded,
        "second run err={:?}",
        parent2_job.error_summary
    );
    let classify_jobs: Vec<_> = matter
        .list_jobs()
        .expect("list")
        .into_iter()
        .filter(|j| j.kind == "classify")
        .collect();
    assert_eq!(
        classify_jobs.len(),
        2,
        "each profile_run creates its own classify child"
    );
}

/// Stage handler with allowlisted kind that fails immediately.
struct FailStageHandler {
    kind: &'static str,
}

impl JobHandler for FailStageHandler {
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

/// Slow allowlisted stage that honors cancel.
struct SlowStageHandler {
    kind: &'static str,
    ticks: Arc<AtomicUsize>,
}

impl JobHandler for SlowStageHandler {
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

/// Fast succeed for allowlisted kind.
struct OkStageHandler {
    kind: &'static str,
}

impl JobHandler for OkStageHandler {
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

#[test]
fn profile_run_stop_on_stage_failure_default() {
    use matter_core::{ProcessingProfileInput, JOB_KIND_PROFILE_RUN};
    use process_runner::MatterProfileRunHandler;

    let (_tmp, base) = utf8_tempdir();
    let root = make_matter(&base, "m-profile-fail");

    {
        let matter = Matter::open(&root).expect("open");
        // classify fails; office_extract would run only if stop_on_stage_failure=false
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
                name: "fail_first".into(),
                description: None,
                body_json: body.into(),
                created_by: None,
            })
            .expect("upsert");
    }

    let mut handler = MatterProfileRunHandler::new();
    handler.register_stage(Arc::new(FailStageHandler { kind: "classify" }));
    handler.register_stage(Arc::new(OkStageHandler {
        kind: "office_extract",
    }));

    let mut runner = ProcessRunner::new(RunnerConfig::default());
    runner.register(Arc::new(handler));

    let parent_id = runner
        .start(
            &root,
            JOB_KIND_PROFILE_RUN,
            JobParams::new(r#"{"profile_name":"fail_first"}"#),
        )
        .expect("start");
    assert!(runner.wait_until_idle(Duration::from_secs(15)));

    let matter = Matter::open(&root).expect("open");
    let parent = matter.get_job(&parent_id).expect("parent");
    assert_eq!(
        parent.state,
        JobState::Failed,
        "default stop_on_stage_failure"
    );
    let office: Vec<_> = matter
        .list_jobs()
        .expect("list")
        .into_iter()
        .filter(|j| j.kind == "office_extract")
        .collect();
    assert!(
        office.is_empty(),
        "office_extract must not run when stop_on_stage_failure=true"
    );
}

#[test]
fn profile_run_continue_on_stage_failure() {
    use matter_core::{ProcessingProfileInput, JOB_KIND_PROFILE_RUN};
    use process_runner::MatterProfileRunHandler;

    let (_tmp, base) = utf8_tempdir();
    let root = make_matter(&base, "m-profile-cont");

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
                name: "cont_fail".into(),
                description: None,
                body_json: body.into(),
                created_by: None,
            })
            .expect("upsert");
    }

    let mut handler = MatterProfileRunHandler::new();
    handler.register_stage(Arc::new(FailStageHandler { kind: "classify" }));
    handler.register_stage(Arc::new(OkStageHandler {
        kind: "office_extract",
    }));

    let mut runner = ProcessRunner::new(RunnerConfig::default());
    runner.register(Arc::new(handler));

    let parent_id = runner
        .start(
            &root,
            JOB_KIND_PROFILE_RUN,
            JobParams::new(r#"{"profile_name":"cont_fail","stop_on_stage_failure":false}"#),
        )
        .expect("start");
    assert!(runner.wait_until_idle(Duration::from_secs(15)));

    let matter = Matter::open(&root).expect("open");
    let parent = matter.get_job(&parent_id).expect("parent");
    assert_eq!(
        parent.state,
        JobState::Succeeded,
        "continues after stage failure when stop_on_stage_failure=false"
    );
    let jobs = matter.list_jobs().expect("list");
    let kinds: std::collections::BTreeSet<_> = jobs.iter().map(|j| j.kind.as_str()).collect();
    assert!(kinds.contains("classify"));
    assert!(kinds.contains("office_extract"));
}

#[test]
fn profile_run_cancel_mid_stage_pauses_parent() {
    use matter_core::{ProcessingProfileInput, JOB_KIND_PROFILE_RUN};
    use process_runner::MatterProfileRunHandler;

    let (_tmp, base) = utf8_tempdir();
    let root = make_matter(&base, "m-profile-cancel");

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
                name: "cancel_mid".into(),
                description: None,
                body_json: body.into(),
                created_by: None,
            })
            .expect("upsert");
    }

    let ticks = Arc::new(AtomicUsize::new(0));
    let mut handler = MatterProfileRunHandler::new();
    handler.register_stage(Arc::new(SlowStageHandler {
        kind: "classify",
        ticks: Arc::clone(&ticks),
    }));
    handler.register_stage(Arc::new(OkStageHandler {
        kind: "office_extract",
    }));

    let mut runner = ProcessRunner::new(RunnerConfig::default());
    runner.register(Arc::new(handler));
    let mut rx = runner.watch_progress();

    let parent_id = runner
        .start(
            &root,
            JOB_KIND_PROFILE_RUN,
            JobParams::new(r#"{"profile_name":"cancel_mid"}"#),
        )
        .expect("start");

    // Wait until the slow stage has made progress, then cancel.
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if ticks.load(Ordering::SeqCst) > 2 {
            break;
        }
        assert!(Instant::now() < deadline, "slow stage never ticked");
        thread::sleep(Duration::from_millis(20));
        let _ = rx.borrow_and_update();
    }
    runner.cancel(&parent_id).expect("cancel");
    assert!(runner.wait_until_idle(Duration::from_secs(15)));

    let paused_classify_id = {
        let matter = Matter::open(&root).expect("open");
        let parent = matter.get_job(&parent_id).expect("parent");
        assert_eq!(
            parent.state,
            JobState::Paused,
            "parent should pause on cancel; err={:?}",
            parent.error_summary
        );
        let office: Vec<_> = matter
            .list_jobs()
            .expect("list")
            .into_iter()
            .filter(|j| j.kind == "office_extract")
            .collect();
        assert!(
            office.is_empty(),
            "office_extract must not run after cancel during classify"
        );

        // Capture paused classify child id before resume.
        let classify: Vec<_> = matter
            .list_jobs()
            .expect("list")
            .into_iter()
            .filter(|j| j.kind == "classify")
            .collect();
        assert_eq!(classify.len(), 1);
        assert_eq!(classify[0].state, JobState::Paused);
        classify[0].id.clone()
        // Drop exclusive write lock before resume/start.
    };

    // Resume: reuses paused classify child (is_resume), then office.
    let mut handler2 = MatterProfileRunHandler::new();
    handler2.register_stage(Arc::new(OkStageHandler { kind: "classify" }));
    handler2.register_stage(Arc::new(OkStageHandler {
        kind: "office_extract",
    }));
    let mut runner2 = ProcessRunner::new(RunnerConfig::default());
    runner2.register(Arc::new(handler2));
    runner2
        .resume(&root, &parent_id)
        .expect("resume profile_run");
    assert!(runner2.wait_until_idle(Duration::from_secs(15)));

    let matter = Matter::open(&root).expect("reopen");
    let parent = matter.get_job(&parent_id).expect("parent");
    assert_eq!(
        parent.state,
        JobState::Succeeded,
        "resume should complete; err={:?}",
        parent.error_summary
    );
    let classify: Vec<_> = matter
        .list_jobs()
        .expect("list")
        .into_iter()
        .filter(|j| j.kind == "classify")
        .collect();
    assert_eq!(
        classify.len(),
        1,
        "resume must reuse paused classify child, not create a second"
    );
    assert_eq!(classify[0].id, paused_classify_id);
    assert_eq!(classify[0].state, JobState::Succeeded);
    let office: Vec<_> = matter
        .list_jobs()
        .expect("list")
        .into_iter()
        .filter(|j| j.kind == "office_extract")
        .collect();
    assert_eq!(office.len(), 1, "office_extract runs after resume");
    assert_eq!(office[0].state, JobState::Succeeded);
}

/// ai_suggest_codes via ProcessRunner + register_default_handlers (Mock matter, no network).
#[cfg(feature = "ai")]
#[test]
fn ai_suggest_codes_handler_via_process_runner() {
    use matter_core::{
        item_role, item_status, ItemInput, UpdateAiMatterConfigInput, AI_PROVIDER_MOCK,
        AI_SUGGESTION_PENDING,
    };
    use process_runner::register_default_handlers;

    let (_tmp, base) = utf8_tempdir();
    let root = make_matter(&base, "m-ai-suggest");

    {
        let matter = Matter::open(&root).expect("open");
        matter
            .update_ai_config(UpdateAiMatterConfigInput {
                enabled: true,
                allow_remote: false,
                base_url: None,
                model: Some("mock"),
                provider_kind: Some(AI_PROVIDER_MOCK),
            })
            .expect("enable ai mock");
        let body = b"this is a hot document for the review team";
        let dig = matter.put_bytes(body).expect("cas");
        matter
            .insert_item(ItemInput {
                status: item_status::EXTRACTED.into(),
                role: Some(item_role::STANDALONE.into()),
                subject: Some("AI runner".into()),
                text_sha256: Some(dig),
                in_review: Some(1),
                ..Default::default()
            })
            .expect("item");
    }

    let mut runner = ProcessRunner::new(RunnerConfig::default());
    register_default_handlers(&mut runner);

    let params = JobParams::new(
        serde_json::json!({
            "scope": "in_review",
            "max_items": 50,
            "reset": false
        })
        .to_string(),
    );
    let job_id = runner
        .start(&root, "ai_suggest_codes", params)
        .expect("start ai_suggest_codes");
    assert!(runner.wait_until_idle(Duration::from_secs(30)));

    let matter = Matter::open(&root).expect("open");
    let job = matter.get_job(&job_id).expect("job");
    assert_eq!(
        job.state,
        JobState::Succeeded,
        "err={:?}",
        job.error_summary
    );
    assert_eq!(job.kind, "ai_suggest_codes");

    let pending = matter.list_pending_ai_suggestions(50).expect("pending");
    assert!(
        !pending.is_empty(),
        "mock should write at least one suggestion"
    );
    assert!(pending.iter().all(|s| s.status == AI_SUGGESTION_PENDING));

    // Job must not write final item_codes.
    let item_id = pending[0].item_id.clone();
    let codes = matter
        .list_item_codes(std::slice::from_ref(&item_id))
        .expect("codes");
    assert!(
        codes[&item_id].is_empty(),
        "ai_suggest_codes must not apply final codes"
    );
}
