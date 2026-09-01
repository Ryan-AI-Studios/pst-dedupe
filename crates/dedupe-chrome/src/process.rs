//! Chrome Process workspace: host `process-runner` + page/start/progress/cancel/resume.

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use matter_core::{
    builtin_profiles, load_case_overview_on, Job, JobState, Matter, OverviewOptions, Source,
};
use process_runner::{JobParams, JobProgressSnapshot, ProcessRunner};
use serde::Serialize;

use crate::error::{map_core, map_runner, CommandError};
use crate::open_root::{open_matter_read, reject_encrypted, utf8_root};
use crate::process_params;

const ALLOWED_KINDS: &[&str] = &["ingest", "extract_pst", "profile_run", "qc", "produce"];
const DEFAULT_PROFILE_ID: &str = "builtin:standard";

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ProcessStartResponse {
    pub job_id: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ProcessSourceRow {
    pub id: String,
    pub path: String,
    pub kind: String,
    pub status: String,
}

impl From<Source> for ProcessSourceRow {
    fn from(s: Source) -> Self {
        Self {
            id: s.id,
            path: s.path,
            kind: s.kind,
            status: s.status,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ProcessPstRow {
    pub id: String,
    pub source_id: Option<String>,
    pub path: Option<String>,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ProcessJobRow {
    pub id: String,
    pub kind: String,
    pub state: String,
    pub parent_job_id: Option<String>,
    pub error_summary: Option<String>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ProcessErrorGroup {
    pub code: String,
    pub count: u64,
    pub sample_message: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct BuiltinProfileFlags {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub classify: bool,
    pub office_extract: bool,
    pub pdf_extract: bool,
    pub ics_extract: bool,
    pub ocr: bool,
    pub fts: bool,
    pub dedupe: bool,
    pub thread: bool,
    pub neardup: bool,
    pub cull: bool,
    pub promote: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ProcessPageResponse {
    pub matter_id: String,
    pub schema_version: u32,
    pub sources: Vec<ProcessSourceRow>,
    pub pst_inventory: Vec<ProcessPstRow>,
    pub jobs: Vec<ProcessJobRow>,
    pub error_groups: Vec<ProcessErrorGroup>,
    pub selected_profile: String,
    pub builtins: Vec<BuiltinProfileFlags>,
    pub discovered: u64,
    pub exceptions: u64,
    pub in_review: u64,
    pub still_processing: u64,
    pub unaccounted_for: u64,
    pub denist: Option<u64>,
    pub dupes: Option<u64>,
    pub families: u64,
}

fn stage_enabled(body: &matter_core::ProfileBody, kind: &str) -> bool {
    body.stages.get(kind).map(|s| s.enabled).unwrap_or(false)
}

fn builtin_flags() -> Vec<BuiltinProfileFlags> {
    builtin_profiles()
        .into_iter()
        .map(|p| BuiltinProfileFlags {
            id: p.id,
            name: p.name,
            description: p.description,
            classify: stage_enabled(&p.body, "classify"),
            office_extract: stage_enabled(&p.body, "office_extract"),
            pdf_extract: stage_enabled(&p.body, "pdf_extract"),
            ics_extract: stage_enabled(&p.body, "ics_extract"),
            ocr: stage_enabled(&p.body, "ocr"),
            fts: stage_enabled(&p.body, "fts_index"),
            dedupe: stage_enabled(&p.body, "dedupe"),
            thread: stage_enabled(&p.body, "thread"),
            neardup: stage_enabled(&p.body, "neardup"),
            cull: stage_enabled(&p.body, "cull"),
            promote: stage_enabled(&p.body, "promote"),
        })
        .collect()
}

const PST_EXTRACT_STAGE: &str = "pst_extract";

fn unaccounted_for(
    inventory_ids: &[String],
    extracted_ids: &HashSet<String>,
    failed_unlogged: u64,
    idle: bool,
) -> u64 {
    // 0 only when idle, every inventory PST id has a successful extract (or there
    // are no PST leaves), *and* no Failed job lacks an item_errors row.
    let pst_gap = if idle && inventory_ids.is_empty() {
        0
    } else {
        inventory_ids
            .iter()
            .filter(|id| !extracted_ids.contains(*id))
            .count() as u64
    };
    pst_gap.saturating_add(failed_unlogged)
}

fn extracted_pst_item_ids(matter: &Matter, jobs: &[Job]) -> Result<HashSet<String>, CommandError> {
    let mut ids = HashSet::new();
    for job in jobs {
        if job.kind != "extract_pst" || job.state != JobState::Succeeded {
            continue;
        }
        let Some(cp) = matter
            .get_checkpoint(&job.id, PST_EXTRACT_STAGE)
            .map_err(map_core)?
        else {
            continue;
        };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&cp.cursor_json) else {
            continue;
        };
        if let Some(id) = v.get("pst_item_id").and_then(|x| x.as_str()) {
            if !id.is_empty() {
                ids.insert(id.to_string());
            }
        }
    }
    Ok(ids)
}

fn failed_jobs_without_item_errors(matter: &Matter, jobs: &[Job]) -> Result<u64, CommandError> {
    let mut n = 0u64;
    for job in jobs {
        if job.state != JobState::Failed {
            continue;
        }
        let errs = matter.item_errors_for_job(&job.id).map_err(map_core)?;
        if errs.is_empty() {
            n += 1;
        }
    }
    Ok(n)
}

fn snapshot_idle_or_terminal(snap: &JobProgressSnapshot) -> bool {
    snap.job_id.is_empty() || snap.state == "idle" || snap.is_terminal()
}

pub fn process_page_blocking(
    runner: &ProcessRunner,
    root: &str,
) -> Result<ProcessPageResponse, CommandError> {
    let matter = open_matter_read(root)?;
    let info = matter
        .info()
        .map_err(|e| CommandError::failed(e.to_string()))?;
    let ov = load_case_overview_on(&matter, &OverviewOptions::default())
        .map_err(|e| CommandError::failed(e.to_string()))?;
    let sources = matter
        .list_sources()
        .map_err(map_core)?
        .into_iter()
        .map(ProcessSourceRow::from)
        .collect();
    let pst_items = matter
        .list_items_by_file_category("pst")
        .map_err(map_core)?;
    let pst_inventory: Vec<ProcessPstRow> = pst_items
        .iter()
        .map(|i| ProcessPstRow {
            id: i.id.clone(),
            source_id: i.source_id.clone(),
            path: i.path.clone(),
            status: i.status.clone(),
        })
        .collect();
    let jobs_raw = matter.list_jobs().map_err(map_core)?;
    let extracted_ids = extracted_pst_item_ids(&matter, &jobs_raw)?;
    let failed_unlogged = failed_jobs_without_item_errors(&matter, &jobs_raw)?;
    let inventory_ids: Vec<String> = pst_inventory.iter().map(|p| p.id.clone()).collect();
    let jobs: Vec<ProcessJobRow> = jobs_raw
        .into_iter()
        .map(|j| ProcessJobRow {
            id: j.id,
            kind: j.kind,
            state: j.state.as_str().to_string(),
            parent_job_id: j.parent_job_id,
            error_summary: j.error_summary,
            started_at: j.started_at,
            finished_at: j.finished_at,
        })
        .collect();
    let recent = matter.list_item_errors_recent(100).map_err(map_core)?;
    let mut grouped: BTreeMap<String, ProcessErrorGroup> = BTreeMap::new();
    for err in recent {
        grouped
            .entry(err.code.clone())
            .and_modify(|g| g.count += 1)
            .or_insert(ProcessErrorGroup {
                code: err.code,
                count: 1,
                sample_message: err.message,
            });
    }
    let error_groups: Vec<ProcessErrorGroup> = grouped.into_values().collect();

    let snap = runner.watch_progress().borrow().clone();
    let same_matter = !snap.matter_id.is_empty() && snap.matter_id == info.id;
    let idle = !same_matter || snapshot_idle_or_terminal(&snap);
    let still_processing = if idle {
        0
    } else {
        match snap.total_hint {
            Some(total) if total >= snap.completed_count => total - snap.completed_count,
            _ => 1,
        }
    };

    let dupes = {
        let counts = matter.count_by_dedup_role().map_err(map_core)?;
        if counts.unique == 0 && counts.duplicate == 0 && counts.skipped == 0 {
            None
        } else {
            Some(counts.duplicate)
        }
    };

    Ok(ProcessPageResponse {
        matter_id: info.id,
        schema_version: info.schema_version,
        sources,
        pst_inventory: pst_inventory.clone(),
        jobs,
        error_groups,
        selected_profile: DEFAULT_PROFILE_ID.into(),
        builtins: builtin_flags(),
        discovered: ov.totals.top_level_items,
        exceptions: ov.errors.total,
        in_review: ov.review.in_review,
        still_processing,
        unaccounted_for: unaccounted_for(&inventory_ids, &extracted_ids, failed_unlogged, idle),
        denist: None,
        dupes,
        families: ov.totals.families_total,
    })
}

/// Process-wide Busy before any write-open (produce/QC preflight).
pub(crate) fn reject_if_busy(runner: &ProcessRunner) -> Result<(), CommandError> {
    if let Some(job) = runner.active_job(None) {
        return Err(CommandError::busy(job.job_id));
    }
    Ok(())
}

pub fn process_start_blocking(
    runner: &ProcessRunner,
    root: &str,
    kind: &str,
    params_json: &str,
) -> Result<ProcessStartResponse, CommandError> {
    let utf8 = utf8_root(root)?;
    reject_encrypted(&utf8)?;
    if kind == "production_export" {
        return Err(CommandError::failed(
            "production_export is a CLI alias; chrome uses produce",
        ));
    }
    if !ALLOWED_KINDS.contains(&kind) {
        return Err(CommandError::failed(format!(
            "kind '{kind}' is not allowed from Process (allowlist: ingest, extract_pst, profile_run, qc, produce)"
        )));
    }
    if !runner.is_registered(kind) {
        return Err(CommandError::failed(format!(
            "handler not registered for kind '{kind}'"
        )));
    }
    let job_id = runner
        .start(&utf8, kind, JobParams::new(params_json.to_string()))
        .map_err(map_runner)?;
    Ok(ProcessStartResponse { job_id })
}

/// Produce already succeeded; do not fail the progress poll on log-repair errors
/// (Finalize latch needs `state==succeeded`). Genuine errors go on `error_summary`.
fn apply_privilege_log_post_step(
    mut snap: JobProgressSnapshot,
    post: Result<(), CommandError>,
) -> JobProgressSnapshot {
    if let Err(e) = post {
        let detail = format!("privilege log: {}", e.message);
        snap.error_summary = Some(match snap.error_summary.take() {
            Some(existing) if !existing.trim().is_empty() => format!("{existing}; {detail}"),
            _ => detail,
        });
    }
    snap
}

pub fn process_progress_blocking(
    runner: &ProcessRunner,
    root: &str,
) -> Result<JobProgressSnapshot, CommandError> {
    let matter = open_matter_read(root)?;
    let matter_id = matter.id().to_string();
    let snap = runner.watch_progress().borrow().clone();
    if snap.job_id.is_empty() || snap.matter_id != matter_id {
        return Ok(JobProgressSnapshot::idle());
    }
    if snap.kind == "produce" && snap.state == "succeeded" {
        let post = crate::produce::ensure_privilege_log_after_produce(root);
        return Ok(apply_privilege_log_post_step(snap, post));
    }
    Ok(snap)
}

pub fn process_cancel_blocking(runner: &ProcessRunner, job_id: &str) -> Result<(), CommandError> {
    runner.cancel(job_id).map_err(map_runner)
}

pub fn process_resume_blocking(
    runner: &ProcessRunner,
    root: &str,
    job_id: &str,
) -> Result<(), CommandError> {
    let utf8 = utf8_root(root)?;
    reject_encrypted(&utf8)?;
    runner.resume(&utf8, job_id).map_err(map_runner)
}

pub fn new_managed_runner() -> Arc<ProcessRunner> {
    let mut runner = ProcessRunner::new(process_runner::RunnerConfig::default());
    process_runner::register_default_handlers(&mut runner);
    Arc::new(runner)
}

#[allow(dead_code)]
pub(crate) fn ingest_params(path: &str) -> String {
    process_params::ingest_params(path)
}

#[allow(dead_code)]
pub(crate) fn extract_pst_item_params(source_id: &str, pst_item_id: &str) -> String {
    process_params::extract_pst_item_params(source_id, pst_item_id)
}

#[allow(dead_code)]
pub(crate) fn profile_run_params(profile_id: &str) -> String {
    process_params::profile_run_params(profile_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::create::create_matter_under;
    use camino::{Utf8Path, Utf8PathBuf};
    use matter_core::{is_encrypted_matter, JobState, Matter, SCHEMA_VERSION};
    use process_runner::{register_default_handlers, ProcessRunner, RunnerConfig};
    use std::collections::HashSet;
    use std::fs::File;
    use std::io::Write;
    use std::time::Duration;
    use tempfile::tempdir;
    use zip::write::SimpleFileOptions;
    use zip::{CompressionMethod, ZipWriter};

    fn utf8_tmp(tmp: &tempfile::TempDir) -> Utf8PathBuf {
        Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).expect("utf8")
    }

    fn test_runner() -> ProcessRunner {
        let mut r = ProcessRunner::new(RunnerConfig::default());
        register_default_handlers(&mut r);
        r
    }

    fn write_tiny_zip(path: &Utf8Path) {
        let file = File::create(path.as_std_path()).expect("zip");
        let mut zip = ZipWriter::new(file);
        let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        zip.start_file("hello.txt", opts).expect("start");
        zip.write_all(b"hello process fold").expect("write");
        zip.finish().expect("finish");
    }

    fn write_slow_zip(path: &Utf8Path) {
        let file = File::create(path.as_std_path()).expect("zip");
        let mut zip = ZipWriter::new(file);
        let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        for i in 0..250 {
            zip.start_file(format!("f{i:03}.txt"), opts).expect("start");
            zip.write_all(&[b'x'; 512]).expect("write");
        }
        zip.finish().expect("finish");
    }

    #[test]
    fn process_module_source_has_no_create_job() {
        let src = include_str!("process.rs");
        let prod = src.split("#[cfg(test)]").next().unwrap_or(src);
        assert!(
            !prod.contains("create_job"),
            "process module must not call create_job (runner Option C)"
        );
    }

    #[test]
    fn schema_stays_41() {
        assert_eq!(SCHEMA_VERSION, 41);
    }

    #[test]
    fn produce_log_post_step_error_keeps_succeeded_snapshot() {
        let mut snap = JobProgressSnapshot::idle();
        snap.job_id = "job_prod".into();
        snap.kind = "produce".into();
        snap.state = "succeeded".into();
        let out = apply_privilege_log_post_step(snap.clone(), Ok(()));
        assert_eq!(out.state, "succeeded");
        assert!(out.error_summary.is_none());
        let out =
            apply_privilege_log_post_step(snap, Err(CommandError::failed("encrypted volume path")));
        assert_eq!(out.state, "succeeded");
        assert_eq!(
            out.error_summary.as_deref(),
            Some("privilege log: encrypted volume path")
        );
    }

    #[test]
    fn builtins_derived_from_live_profile_body() {
        let flags = builtin_flags();
        let standard = flags.iter().find(|p| p.id == "builtin:standard").unwrap();
        assert!(
            standard.classify
                && standard.dedupe
                && standard.thread
                && standard.cull
                && standard.promote
        );
        assert!(!standard.ocr && !standard.neardup && !standard.fts);
        let with_ocr = flags.iter().find(|p| p.id == "builtin:with_ocr").unwrap();
        assert!(with_ocr.ocr);
        assert!(!with_ocr.neardup, "with_ocr must not enable neardup");
    }

    #[test]
    fn is_registered_produce_and_qc_after_default_handlers() {
        let runner = test_runner();
        assert!(runner.is_registered("produce"));
        assert!(runner.is_registered("qc"));
        runner.shutdown();
    }

    #[test]
    fn ingest_tiny_zip_succeeds_and_encrypted_refused() {
        let tmp = tempdir().expect("tempdir");
        let parent = utf8_tmp(&tmp);
        let root = create_matter_under(&parent, "IngestZip").expect("create");
        let zip_path = parent.join("tiny.zip");
        write_tiny_zip(&zip_path);

        let runner = test_runner();
        let started = process_start_blocking(
            &runner,
            root.as_str(),
            "ingest",
            &ingest_params(zip_path.as_str()),
        )
        .expect("start ingest");
        assert!(!started.job_id.is_empty());
        assert!(runner.wait_until_idle(Duration::from_secs(60)));
        let matter = Matter::open_for_read(&root).expect("read");
        let sources = matter.list_sources().expect("sources");
        assert!(
            !sources.is_empty(),
            "ingest must register at least one source"
        );
        let page = process_page_blocking(&runner, root.as_str()).expect("page");
        assert_eq!(page.schema_version, 41);
        assert_eq!(page.unaccounted_for, 0, "txt zip has no PST leaves");

        let enc_root = parent.join("EncProc");
        {
            let _m = Matter::create_encrypted(&enc_root, "EncProc", "test-passphrase-0116")
                .expect("enc");
        }
        assert!(is_encrypted_matter(&enc_root));
        let err = process_start_blocking(
            &runner,
            enc_root.as_str(),
            "ingest",
            &ingest_params(zip_path.as_str()),
        )
        .expect_err("encrypted");
        assert_eq!(err.kind, "encrypted");
        let page_err = process_page_blocking(&runner, enc_root.as_str()).expect_err("page enc");
        assert_eq!(page_err.kind, "encrypted");
        runner.shutdown();
    }

    #[test]
    fn second_start_while_running_is_busy() {
        let tmp = tempdir().expect("tempdir");
        let parent = utf8_tmp(&tmp);
        let root = create_matter_under(&parent, "Busy").expect("create");
        let zip_path = parent.join("slow.zip");
        write_slow_zip(&zip_path);
        let runner = test_runner();
        let first = process_start_blocking(
            &runner,
            root.as_str(),
            "ingest",
            &ingest_params(zip_path.as_str()),
        )
        .expect("first");
        let second = process_start_blocking(
            &runner,
            root.as_str(),
            "ingest",
            &ingest_params(zip_path.as_str()),
        );
        match second {
            Err(e) => {
                assert_eq!(e.kind, "busy");
                assert!(e.message.contains(&first.job_id));
            }
            Ok(_) => {
                // Tiny race: first already finished. Durable Running fallback.
                let matter = Matter::open(&root).expect("open");
                let job = matter.create_job("ingest").expect("job");
                matter
                    .set_job_state(&job.id, JobState::Running, None)
                    .expect("running");
                drop(matter);
                let again = process_start_blocking(
                    &runner,
                    root.as_str(),
                    "profile_run",
                    &profile_run_params("builtin:standard"),
                )
                .expect_err("busy durable");
                assert_eq!(again.kind, "busy");
            }
        }
        let _ = runner.cancel(&first.job_id);
        let _ = runner.wait_until_idle(Duration::from_secs(60));
        runner.shutdown();
    }

    #[test]
    fn produce_start_while_ingest_running_is_busy() {
        let tmp = tempdir().expect("tempdir");
        let parent = utf8_tmp(&tmp);
        let root = create_matter_under(&parent, "ProdBusy").expect("create");
        let zip_path = parent.join("slow.zip");
        write_slow_zip(&zip_path);
        let runner = test_runner();
        let first = process_start_blocking(
            &runner,
            root.as_str(),
            "ingest",
            &ingest_params(zip_path.as_str()),
        )
        .expect("ingest");
        let err = crate::produce::produce_start_blocking(
            &runner,
            crate::produce::ProduceStartArgs {
                root: root.as_str().into(),
                filter_json: None,
                item_ids: None,
                production_profile: None,
                source_entire_corpus: None,
                bates_prefix: Some("PROD".into()),
                bates_start: Some(1),
                warning_overrides: None,
                log_format: Some("standard".into()),
                last_findings: None,
            },
        )
        .expect_err("busy");
        assert_eq!(err.kind, "busy");
        assert!(err.message.contains(&first.job_id));
        let _ = runner.cancel(&first.job_id);
        let _ = runner.wait_until_idle(Duration::from_secs(60));
        runner.shutdown();
    }

    #[test]
    fn cancel_cooperative_pauses_or_cancels() {
        let tmp = tempdir().expect("tempdir");
        let parent = utf8_tmp(&tmp);
        let root = create_matter_under(&parent, "Cancel").expect("create");
        let zip_path = parent.join("slow.zip");
        write_slow_zip(&zip_path);
        let runner = test_runner();
        let started = process_start_blocking(
            &runner,
            root.as_str(),
            "ingest",
            &ingest_params(zip_path.as_str()),
        )
        .expect("start");
        process_cancel_blocking(&runner, &started.job_id).expect("cancel");
        assert!(runner.wait_until_idle(Duration::from_secs(60)));
        let snap = runner.watch_progress().borrow().clone();
        assert!(
            snap.state == "paused" || snap.state == "cancelled" || snap.state == "succeeded",
            "cancel vocabulary: got {}",
            snap.state
        );
        runner.shutdown();
    }

    #[test]
    fn production_export_rejected() {
        let tmp = tempdir().expect("tempdir");
        let parent = utf8_tmp(&tmp);
        let root = create_matter_under(&parent, "Export").expect("create");
        let runner = test_runner();
        let err = process_start_blocking(&runner, root.as_str(), "production_export", "{}")
            .expect_err("reject");
        assert_eq!(err.kind, "failed");
        assert!(err.message.contains("produce"));
        runner.shutdown();
    }

    #[test]
    fn process_progress_isolates_by_matter_id() {
        let tmp = tempdir().expect("tempdir");
        let parent = utf8_tmp(&tmp);
        let root_a = create_matter_under(&parent, "ProgA").expect("a");
        let root_b = create_matter_under(&parent, "ProgB").expect("b");
        let zip_path = parent.join("tiny.zip");
        write_tiny_zip(&zip_path);
        let runner = test_runner();
        let _ = process_start_blocking(
            &runner,
            root_a.as_str(),
            "ingest",
            &ingest_params(zip_path.as_str()),
        )
        .expect("start a");
        let other = process_progress_blocking(&runner, root_b.as_str()).expect("b");
        assert_eq!(other.state, "idle");
        assert!(other.job_id.is_empty());
        let _ = runner.wait_until_idle(Duration::from_secs(60));
        runner.shutdown();
    }

    #[test]
    fn crash_recovery_resume_orphan_running() {
        let tmp = tempdir().expect("tempdir");
        let parent = utf8_tmp(&tmp);
        let root = create_matter_under(&parent, "Orphan").expect("create");
        let zip_path = parent.join("tiny.zip");
        write_tiny_zip(&zip_path);

        let orphan_id = {
            let matter = Matter::open(&root).expect("open");
            let job = matter.create_job("ingest").expect("job");
            matter
                .set_job_state(&job.id, JobState::Running, None)
                .expect("orphan running");
            job.id
        };

        let runner = test_runner();
        let snap = process_progress_blocking(&runner, root.as_str()).expect("idle snap");
        assert_eq!(snap.state, "idle");

        let busy = process_start_blocking(
            &runner,
            root.as_str(),
            "ingest",
            &ingest_params(zip_path.as_str()),
        )
        .expect_err("busy while orphan running");
        assert_eq!(busy.kind, "busy");

        process_resume_blocking(&runner, root.as_str(), &orphan_id)
            .expect("resume same-id Running orphan");
        assert!(
            runner.wait_until_idle(Duration::from_secs(60)),
            "orphan resume must reach idle (handler may fail the job; that still unblocks start)"
        );

        let started = process_start_blocking(
            &runner,
            root.as_str(),
            "ingest",
            &ingest_params(zip_path.as_str()),
        )
        .expect("later start unblocked");
        assert!(!started.job_id.is_empty());
        let _ = runner.wait_until_idle(Duration::from_secs(60));
        runner.shutdown();
    }

    #[test]
    fn golden_path_ingest_profile_unaccounted_zero() {
        let tmp = tempdir().expect("tempdir");
        let parent = utf8_tmp(&tmp);
        let root = create_matter_under(&parent, "Golden").expect("create");
        let zip_path = parent.join("tiny.zip");
        write_tiny_zip(&zip_path);
        let runner = test_runner();

        process_start_blocking(
            &runner,
            root.as_str(),
            "ingest",
            &ingest_params(zip_path.as_str()),
        )
        .expect("ingest");
        assert!(runner.wait_until_idle(Duration::from_secs(60)));

        let page = process_page_blocking(&runner, root.as_str()).expect("page after ingest");
        assert_eq!(page.unaccounted_for, 0);
        assert!(page.builtins.iter().any(|b| b.id == "builtin:standard"));

        let pst_path = {
            let mut p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            p.pop();
            p.pop();
            p.join("fixtures").join("aspose_outlook.pst")
        };
        if pst_path.is_file() {
            let utf_pst = Utf8PathBuf::from_path_buf(pst_path).expect("utf8 pst");
            process_start_blocking(
                &runner,
                root.as_str(),
                "ingest",
                &ingest_params(utf_pst.as_str()),
            )
            .expect("ingest pst");
            assert!(runner.wait_until_idle(Duration::from_secs(120)));
            let page2 = process_page_blocking(&runner, root.as_str()).expect("page pst");
            for pst in &page2.pst_inventory {
                let sid = pst.source_id.as_deref().unwrap_or("");
                process_start_blocking(
                    &runner,
                    root.as_str(),
                    "extract_pst",
                    &extract_pst_item_params(sid, &pst.id),
                )
                .expect("extract");
                assert!(runner.wait_until_idle(Duration::from_secs(180)));
            }
            process_start_blocking(
                &runner,
                root.as_str(),
                "profile_run",
                &profile_run_params("builtin:extract_only"),
            )
            .expect("profile");
            assert!(runner.wait_until_idle(Duration::from_secs(180)));
            let page3 = process_page_blocking(&runner, root.as_str()).expect("page3");
            assert_eq!(page3.unaccounted_for, 0);
            assert!(
                page3.discovered > 0,
                "extract should yield top_level_items > 0"
            );
        } else {
            process_start_blocking(
                &runner,
                root.as_str(),
                "profile_run",
                &profile_run_params("builtin:extract_only"),
            )
            .expect("profile txt");
            assert!(runner.wait_until_idle(Duration::from_secs(120)));
            let page3 = process_page_blocking(&runner, root.as_str()).expect("page txt");
            assert_eq!(page3.unaccounted_for, 0);
        }
        runner.shutdown();
    }

    #[test]
    fn process_ui_is_live_not_stub() {
        let src = include_str!("../ui/src/pages/process.rs");
        assert!(!src.contains("Process stays in Dedupe Desk until 0116."));
        assert!(src.contains(
            "Processing is deterministic. No prediction, no coding, no privilege calls here."
        ));
        let ui_toml = include_str!("../ui/Cargo.toml");
        assert!(!ui_toml.contains("process-runner"));
    }

    #[test]
    fn unaccounted_nonzero_when_pst_inventory_without_extract() {
        let none = HashSet::new();
        let both: HashSet<String> = ["a".into(), "b".into()].into_iter().collect();
        let only_a: HashSet<String> = ["a".into()].into_iter().collect();
        assert_eq!(
            unaccounted_for(&["a".into(), "b".into()], &none, 0, true),
            2
        );
        assert_eq!(
            unaccounted_for(&["a".into(), "b".into()], &both, 0, true),
            0
        );
        assert_eq!(unaccounted_for(&[], &none, 0, true), 0);
        assert!(unaccounted_for(&["a".into()], &none, 0, false) > 0);
        assert_eq!(unaccounted_for(&[], &none, 1, true), 1);
        assert_eq!(
            unaccounted_for(&["a".into(), "b".into()], &both, 1, true),
            1
        );
        assert_eq!(
            unaccounted_for(&["a".into(), "b".into()], &only_a, 0, true),
            1,
            "extracting A twice must not zero-out missing B"
        );
    }
}
