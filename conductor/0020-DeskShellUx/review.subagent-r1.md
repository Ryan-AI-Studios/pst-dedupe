# 0020 Internal Review R1 — DeskShellUx

## Verdict: NEEDS_FIX

Static, read-only audit of track **0020-DeskShellUx** on branch `feat/0020-deskshellux`.  
Gates (`cargo test`, clippy, `ledgerful verify`, manual GUI smoke) were **not executed** in this pass — matrix marks them Not verifiable where required.

---

## Scope Reviewed

| Area | Paths |
|---|---|
| Spec / plan | `conductor/0020-DeskShellUx/spec.md`, `plan.md` |
| Desk crate | `crates/dedupe-desk/**` (`main`, `app`, `matter_ui`, `workspace`, `progress_ui`, `dialogs`, `nav`, `params`, `settings`, README, Cargo.toml) |
| matter-core APIs | `crates/matter-core/src/matter.rs` (`list_sources`, `count_items`, `list_items_by_file_category`, `open_for_read`), schema WAL, integration tests |
| process-runner contract | Public API: `start` / `resume` / `cancel` / `watch_progress` / `shutdown` / `Busy`; handlers register |
| Workspace / docs | Root `Cargo.toml` member, `README.md`, `ARCHITECTURE.md` |
| Completeness sweep | `TODO`/`FIXME`/`todo!`/`unimplemented!` under `dedupe-desk` (none blocking; stub nav intentional) |

---

## Summary Table

| ID | Severity | Title | Status |
|---|---|---|---|
| R1-P1 | high | `Matter::open` (temp wipe) allowed while process-runner may be writing | open |
| R1-P2 | medium | Durable `Busy` (leftover Running job) has no Resume path; copy says Cancel/wait | open |
| R1-P3 | medium | SQLITE_BUSY soft-path matches `"busy"` but rusqlite text is usually `"database is locked"` | open |
| R1-P4 | medium | Extract-all queue drops target when `start` fails after `pop_front` | open |
| R1-P5 | low | Home “Recent → Open” ignores `dialog_open` debounce | open |
| R1-P6 | low | `runner_busy` third clause is dead; pending extract queue not treated as busy | open |
| R1-P7 | low | `last_parent_dir` persisted but never used for rfd initial directory | open |

**Overall:** Core shell is largely wired (Option B binary, ProcessRunner-only heavy work, off-thread rfd + debounce, throttled repaint, `open_for_read` refresh, shutdown on exit, docs). **Not CLEAN** due to concurrent open/temp race (R1-P1) and Busy/queue/busy-string gaps (R1-P2–P4).

---

## DoD Matrix

| DoD | Status | Evidence | Tests | Gap |
|---|---|---|---|---|
| **DoD-1 — Single-exe Desk** | Met (static) | Workspace member `crates/dedupe-desk`; `[[bin]] name = "dedupe-desk"`; eframe shell title “Dedupe Desk”; `pst-dedup-gui` still a member | Unit tests in crate | Gates not run here |
| **DoD-2 — Matter UX** | Met | Home: create (name + parent folder), open folder, recent; errors via `error_msg` banner | `matter_ui` create/open/WAL; `params` name validation | — |
| **DoD-3 — Sources + process** | Met | Add folder/ZIP/PST → `start("ingest", {path})`; Extract selected/all → `start("extract_pst", {source_id, pst_item_id})`; handlers registered | params JSON shape tests | E2E ingest/extract not automated in desk |
| **DoD-4 — Progress + cancel** | Met (static) | `watch_progress` each frame; Cancel/Resume; `request_repaint_after(100ms)` only (no free-run `request_repaint()`) | `progress_ui::job_is_active` | Manual cancel/resume smoke not observed |
| **DoD-5 — UI thread safety** | Partial | No `ingest_path`/`extract_pst_*` on UI; rfd on `desk-rfd` thread; `dialog_open` gate on Add/Create/Open | dialog spawn-while-open no-op | **R1-P1**: `Matter::open` wipe on UI can race worker temp; **R1-P5** recent Open |
| **DoD-6 — Concurrent refresh** | Partial | Refresh via `open_for_read`; WAL asserted in unit test; soft status intended for busy | `create_open_refresh_and_wal` | **R1-P3** busy string; no concurrent read-during-write automated test; no `busy_timeout` |
| **DoD-7 — Shutdown** | Met | `on_exit` + `DeskApp::Drop` call `runner.shutdown()`; runner Drop also joins | — | Not runtime-observed |
| **DoD-8 — Docs + smoke** | Met (static) | Root README Desk binary; crate README (UI rules, 100ms, dialog, WAL, smoke steps); ARCHITECTURE Desk shell section | — | Manual smoke not observed |
| **DoD-9 — Workspace gate** | Not verifiable | Spec commands listed | — | Orchestrator must run fmt/clippy/test/`ledgerful verify` |
| **DoD-10 — Recorded** | Unmet (expected pre-finalize) | No canonical `review.md`; conductor **In Progress**; ledger finalize open | — | Finalize after fixes + gates |

### Spec §3 / design locks

| Lock | Status | Notes |
|---|---|---|
| Option B `dedupe-desk` | Met | New crate; legacy GUI retained |
| eframe 0.34 / rfd 0.17 | Met | Workspace pins; desk uses workspace deps |
| ProcessRunner only for heavy work | Met | Ingest + ExtractPst handlers registered |
| rfd off-thread + `dialog_open` | Met (mostly) | `DialogState`; pickers disabled while open; **R1-P5** |
| `request_repaint_after(100ms)` | Met | `progress_ui::REPAINT_WHILE_JOB_MS = 100`; dialog poll also 100ms |
| UI lists `open_for_read` | Met | `refresh_snapshot` |
| Busy handling | Partial | Product copy for in-process Busy; durable leftover weak (**R1-P2**) |
| Extract sequential queue | Partial | Implemented; failure drop (**R1-P4**) |
| Stub Reduce/Review/Produce | Met | “Coming soon” |
| Settings last path | Met | Recent + `last_parent_dir` (unused for picker — R1-P7) |

---

## Findings

### R1-P1

- **id:** R1-P1  
- **severity:** high  
- **description:** Opening a matter always calls `Matter::open`, which **deletes `workspace/temp/`**. There is no guard against doing this while `ProcessRunner` is mid-ingest/extract on the same (or any) matter. Home remains fully usable during a job; “Open matter folder…” and “Recent → Open” both go through `open_matter_at` → `Matter::open`. That races CAS materialization / extract spill under `workspace/temp/` (exactly the hazard `open_for_read` exists to avoid).  
- **source:** Spec §2.5 / §3.1 (prefer `open_for_read` for concurrent UI; temp wipe only on primary open); DoD-5 / DoD-6; matter-core docs on `open` vs `open_for_read`  
- **files:**  
  - `C:\dev\Dedupe\crates\dedupe-desk\src\matter_ui.rs` (`open_matter`)  
  - `C:\dev\Dedupe\crates\dedupe-desk\src\app.rs` (`open_matter_at`, `set_matter`, Home recent/open; no busy guard)  
  - `C:\dev\Dedupe\crates\matter-core\src\matter.rs` (`open` cleans temp; `open_for_read` does not)  
- **required_fix:**  
  1. Block create/open/switch matter while `runner.is_busy()` or progress `state == "running"` (clear copy: finish/cancel first), **and/or**  
  2. Validate open with `Matter::open_for_read` (or open only when idle with `Matter::open` for orphan cleanup).  
  3. Prefer cancel-or-wait before switching `matter_root` under an active job.  
- **status:** open  
- **evidence:**

```17:20:crates/dedupe-desk/src/matter_ui.rs
pub fn open_matter(root: &Utf8Path) -> Result<String, String> {
    let matter = Matter::open(root).map_err(|e| e.to_string())?;
    let info = matter.info().map_err(|e| e.to_string())?;
```

```285:291:crates/matter-core/src/matter.rs
/// **Do not** call this from a concurrent progress/status poller while
/// another handle is extracting: temp cleanup would race CAS materialization.
/// Use [`Matter::open_for_read`] for concurrent readers.
pub fn open(root: impl AsRef<Utf8Path>) -> Result<Self> {
```

Home Open/Recent always call `open_matter_at` with no `runner_busy()` check (`app.rs` ~158–172, ~388–394).

---

### R1-P2

- **id:** R1-P2  
- **severity:** medium  
- **description:** When process-runner returns durable `Busy` (leftover `JobState::Running` row after crash — tested in process-runner), Desk shows “A job is already running. Cancel or wait.” That guidance is wrong for durable Busy: there is no in-process active job to cancel, and waiting never clears the row. Resume is only enabled when the **watch** snapshot is `paused`/`failed` with a non-empty `job_id`. Idle watch after restart leaves Resume disabled; jobs table is display-only (no select/resume). process-runner **does** allow `resume` of durable `Running` jobs, but Desk never offers that path.  
- **source:** Spec §3.4.8 Busy handling; §6 risk “Busy / crash leftover Running job — UI surfaces Busy + guidance to resume or mark failed”; DoD-4 resume path  
- **files:**  
  - `C:\dev\Dedupe\crates\dedupe-desk\src\params.rs` (`format_runner_error`)  
  - `C:\dev\Dedupe\crates\dedupe-desk\src\workspace.rs` (`can_resume`)  
  - `C:\dev\Dedupe\crates\dedupe-desk\src\app.rs` (`resume_active`, start error path does not seed `last_job_id` from Busy)  
- **required_fix:** On `RunnerError::Busy { job_id }`, set `last_job_id`, surface message to **Resume job {id}** (or pick from jobs table). Enable Resume for durable Running/Paused/Failed job rows from the jobs list (or auto-offer resume of Busy id). Do not claim Cancel/wait for durable-only Busy.  
- **status:** open  
- **evidence:**

```61:68:crates/dedupe-desk/src/params.rs
pub fn format_runner_error(err: &process_runner::RunnerError) -> String {
    match err {
        process_runner::RunnerError::Busy { job_id } => {
            format!("A job is already running. Cancel or wait. (job {job_id})")
        }
```

```89:91:crates/dedupe-desk/src/workspace.rs
        let can_resume = (snap.state == "paused" || snap.state == "failed") && !job_id.is_empty();
```

process-runner allows resume of `JobState::Running` (`runner.rs` ~553–554); durable Busy test: `durable_running_job_blocks_second_start`.

---

### R1-P3

- **id:** R1-P3  
- **severity:** medium  
- **description:** Live refresh soft-handles SQLITE_BUSY only when the error string contains `"busy"`. matter-core surfaces rusqlite errors as `SQLite error: …`; SQLITE_BUSY commonly displays as **`database is locked`**, which does **not** contain `"busy"`. Those failures go to the red **Error** banner (`error_msg`) instead of soft status + periodic retry, undercutting DoD-6 “do not hard-fail … SQLITE_BUSY”. There is also no explicit one-shot immediate retry/backoff (only the 2s running refresh loop, and only if the first failure was classified soft).  
- **source:** Spec §2.5.3 (retry/backoff if transient busy); DoD-6  
- **files:** `C:\dev\Dedupe\crates\dedupe-desk\src\app.rs` (`refresh_matter_lists`)  
- **required_fix:** Treat locked/busy/SQLITE_BUSY/`ErrorCode::DatabaseBusy` (or match `"locked"` / `"busy"`) as soft; optionally one immediate retry with short sleep; keep periodic refresh. Prefer not to set `error_msg` for transient lock.  
- **status:** open  
- **evidence:**

```114:120:crates/dedupe-desk/src/app.rs
            Err(e) => {
                // Transient SQLITE_BUSY under load: soft status, not hard fail.
                if e.to_lowercase().contains("busy") {
                    self.status_msg = Some("Matter busy; will retry refresh…".into());
                } else {
                    self.error_msg = Some(format!("Refresh failed: {e}"));
                }
```

matter-core: `Error::Sqlite(#[from] rusqlite::Error)` display `"SQLite error: {0}"` — no dedicated Busy variant; no `busy_timeout` in `configure_connection`.

---

### R1-P4

- **id:** R1-P4  
- **severity:** medium  
- **description:** Sequential extract-all pops the next target **before** `start` succeeds. If `start` returns `Busy` or any other error, that PST is removed from the queue and not re-queued; remaining items may still sit in the queue without an automatic pump (state never transitioned from `running`). Operator must re-click Extract all; one target is silently skipped unless they notice the error banner.  
- **source:** Spec §3.4.3 Extract all sequential queue; plan Phase 4  
- **files:** `C:\dev\Dedupe\crates\dedupe-desk\src\app.rs` (`pump_extract_queue`, `start_extract_one`)  
- **required_fix:** Only `pop_front` after successful `start`, or push back on `Err`; on failure clear or pause the queue with explicit status.  
- **status:** open  
- **evidence:**

```256:263:crates/dedupe-desk/src/app.rs
    fn pump_extract_queue(&mut self) {
        if self.runner.is_busy() || self.progress_rx.borrow().state == "running" {
            return;
        }
        if let Some(next) = self.extract_queue.pop_front() {
            self.start_extract_one(next.source_id, next.pst_item_id);
        }
    }
```

`start_extract_one` on `Err` only sets `error_msg` — does not restore queue entry.

---

### R1-P5

- **id:** R1-P5  
- **severity:** low  
- **description:** Create/Open folder pickers honor `dialog.is_open()`, but **Recent → Open** buttons do not. User can open a recent matter (or spam Open) while an rfd thread is still active, partially defeating multi-dialog / concurrent action debounce intent.  
- **source:** Spec §3.3.1 dialog debounce; DoD-5  
- **files:** `C:\dev\Dedupe\crates\dedupe-desk\src\app.rs` (`show_home` recent loop)  
- **required_fix:** Disable Recent Open (and optionally Create text path) while `dialog.is_open()`.  
- **status:** open  
- **evidence:**

```388:394:crates/dedupe-desk/src/app.rs
            for path in recent {
                ui.horizontal(|ui| {
                    if ui.button("Open").clicked() {
                        self.open_matter_at(PathBuf::from(&path));
                    }
```

vs Create/Open folder using `!self.dialog.is_open()`.

---

### R1-P6

- **id:** R1-P6  
- **severity:** low  
- **description:** `runner_busy()` third disjunct is logically redundant with the second (`state == "running"`). A non-empty `extract_queue` while state is not `running` (e.g. brief gap if pump fails, or leftover after failed start) does not mark busy, so Start/Add can be re-enabled while queue still holds work. Low practical impact if R1-P4 fixed and pump always runs on success.  
- **source:** Spec §3.4.3 sequential extract; plan Busy disable  
- **files:** `C:\dev\Dedupe\crates\dedupe-desk\src\app.rs` (`runner_busy`)  
- **required_fix:** Treat `!extract_queue.is_empty()` as busy (or “queue draining”); remove redundant clause.  
- **status:** open  
- **evidence:**

```80:84:crates/dedupe-desk/src/app.rs
    pub(crate) fn runner_busy(&self) -> bool {
        self.runner.is_busy()
            || self.progress_rx.borrow().state == "running"
            || !self.extract_queue.is_empty() && self.progress_rx.borrow().state == "running"
    }
```

---

### R1-P7

- **id:** R1-P7  
- **severity:** low  
- **description:** Settings save `last_parent_dir` on create but never pass it to rfd (`set_directory`). Spec marks last-path persistence recommended; recent matters work. Incomplete wiring only.  
- **source:** Spec §3.2 / §3.7 settings  
- **files:** `C:\dev\Dedupe\crates\dedupe-desk\src\settings.rs`, `dialogs.rs`, `app.rs`  
- **required_fix:** Optional: seed Create parent folder dialog with `last_parent_dir`.  
- **status:** open  
- **evidence:** `last_parent_dir` written in `create_matter_at`; no reads outside struct definition/load/save.

---

## Completeness Sweep

| Check | Result |
|---|---|
| `TODO`/`FIXME`/`todo!`/`unimplemented!` in desk | None functional |
| Stub Reduce/Review/Produce | Intentional placeholders (spec §3.1 / out of scope) |
| `extract_pst_path_params` | `#[allow(dead_code)]` helper; UI uses inventory form (spec-allowed) |
| Dead wiring | ProcessRunner handlers registered; workspace actions call start/cancel/resume |
| Fake progress | Fraction may be indeterminate oscillation when no `total_hint` — honest enough; mid-run `completed_count` depends on 0019 poller (out of 0020 scope if already fixed in runner) |
| Free-run repaint | **None** in desk sources (only `request_repaint_after`) |
| Stage APIs on UI | **None** |

---

## Wiring / Regression Notes (no extra findings)

| Path | Wired? |
|---|---|
| Create matter → `Matter::create` → Workspace | Yes |
| Open / Recent → validate matter → Workspace | Yes (see R1-P1) |
| Add * → off-thread rfd → `start(ingest)` | Yes |
| Extract selected/all → `start(extract_pst)` + queue | Yes (R1-P4) |
| Progress panel ← `watch_progress` | Yes |
| Cancel / Resume | Yes for live watch job_id / last_job_id |
| Lists/stats ← `open_for_read` + `list_sources` / `list_items_by_file_category("pst")` / `list_jobs` / `count_items` | Yes; matter-core APIs + integration test present |
| journal_mode shown in Counts | Yes |
| Exit → shutdown/join | Yes (`on_exit` + Drop) |
| Root README / ARCHITECTURE / crate README | Updated for Desk |

**Rust idioms:** Reasonable module split; pure helpers unit-tested; minor dead clause (R1-P6); double shutdown is idempotent (OK).

---

## Verification Evidence

| Item | Observed now |
|---|---|
| Static code review of desk + matter-core list APIs + docs | Yes |
| `cargo test -p dedupe-desk` / workspace / clippy / fmt | **Not run** |
| `ledgerful verify` | **Not run** |
| Manual GUI smoke (create/ingest/extract/cancel/resume) | **Not run** |
| Concurrent UI refresh during job | **Not run** (unit WAL only) |

---

## Completion Decision

**NEEDS_FIX** — implement R1-P1 (required) and R1-P2–P4 before treating engineering DoD as met. R1-P5–P7 are easy polish. Then re-run gates + manual smoke; finalize DoD-9/10 separately.

Do **not** mark conductor **Completed** on this R1 alone.
