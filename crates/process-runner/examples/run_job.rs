//! Smoke example: run a mock-style ingest via ProcessRunner.
//!
//! ```powershell
//! cargo run -p process-runner --example run_job -- path\to\package_or_zip
//! ```

use std::env;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use camino::Utf8PathBuf;
use matter_core::Matter;
use process_runner::{IngestHandler, JobParams, ProcessRunner, RunnerConfig};
use tempfile::tempdir;

fn main() {
    let args: Vec<String> = env::args().collect();
    let package = args.get(1).map(String::as_str).unwrap_or("");

    let tmp = tempdir().expect("tempdir");
    let matter_root = Utf8PathBuf::from_path_buf(tmp.path().join("matter")).expect("utf8");
    Matter::create(&matter_root, "example").expect("create matter");

    let mut runner = ProcessRunner::new(RunnerConfig::default());
    runner.register(Arc::new(IngestHandler::new()));
    let mut progress = runner.watch_progress();

    if package.is_empty() {
        eprintln!("usage: run_job <path-to-zip-or-package>");
        eprintln!("(no path given — demonstrating unknown-kind / idle watch only)");
        let snap = progress.borrow().clone();
        eprintln!("idle snapshot state={}", snap.state);
        runner.shutdown();
        return;
    }

    let params = JobParams::new(serde_json::json!({ "path": package }).to_string());
    match runner.start(&matter_root, "ingest", params) {
        Ok(job_id) => {
            eprintln!("started job {job_id}");
            while runner.is_busy() {
                let snap = progress.borrow_and_update().clone();
                eprintln!(
                    "progress: state={} count={} msg={:?}",
                    snap.state, snap.completed_count, snap.message
                );
                thread::sleep(Duration::from_millis(200));
            }
            let snap = progress.borrow().clone();
            eprintln!(
                "terminal: job={} state={} msg={:?}",
                snap.job_id, snap.state, snap.message
            );
        }
        Err(e) => eprintln!("start failed: {e}"),
    }

    runner.shutdown();
}
