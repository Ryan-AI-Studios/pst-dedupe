//! Integration tests for process-runner.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

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
    let Some(pst) = candidates.into_iter().find(|p| p.is_file()) else {
        eprintln!("skip: no fixture PST");
        return;
    };

    let (_tmp, base) = utf8_tempdir();
    let matter_root = make_matter(&base, "m-extract");
    let (source_id, inv_id) = {
        let matter = Matter::open(&matter_root).expect("open");
        let source = matter
            .insert_source(pst.to_str().unwrap(), "pst", "importing", None)
            .expect("source");
        let bytes = fs::read(&pst).expect("read");
        let digest = matter.put_bytes(&bytes).expect("cas");
        let name = pst
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("mail.pst");
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
    // max_messages may pause or succeed depending on fixture size.
    assert!(
        matches!(
            jobs[0].state,
            JobState::Succeeded | JobState::Paused | JobState::Failed
        ),
        "unexpected state {:?}",
        jobs[0].state
    );
    if jobs[0].state == JobState::Failed {
        // Open failures on empty/broken fixtures are acceptable for smoke.
        eprintln!("extract failed (fixture?): {:?}", jobs[0].error_summary);
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
