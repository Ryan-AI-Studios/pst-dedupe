# 0020 Internal Review R2 — DeskShellUx (re-review)

## Verdict: CLEAN

Static, read-only re-review of track **0020-DeskShellUx** on branch `feat/0020-deskshellux` after R1 fixes.  
Scope: verify every R1 finding against current `crates/dedupe-desk/**` (and related call sites); sweep for regressions / new issues.  
Gates (`cargo test`, clippy, `ledgerful verify`, manual GUI smoke) were **not executed** in this pass.

---

## Scope Reviewed

| Area | Paths |
|---|---|
| Prior findings | `conductor/0020-DeskShellUx/review.subagent-r1.md` |
| Desk crate | `crates/dedupe-desk/**` (`app`, `matter_ui`, `workspace`, `params`, `dialogs`, `settings`, `progress_ui`, `nav`, `main`, README, Cargo.toml) |
| Cross-check | process-runner `RunnerError` / `start` / `resume` surface; root README / ARCHITECTURE Desk mentions |
| Completeness | `TODO`/`FIXME`/`todo!`/`unimplemented!` under `dedupe-desk` — none |

---

## R1 Finding Disposition

| ID | Severity (R1) | Title | R2 status |
|---|---|---|---|
| R1-P1 | high | `Matter::open` temp wipe while runner may be writing | **verified_fixed** |
| R1-P2 | medium | Durable `Busy` has no Resume path; Cancel/wait copy | **verified_fixed** |
| R1-P3 | medium | SQLITE_BUSY soft-path misses `"database is locked"` | **verified_fixed** |
| R1-P4 | medium | Extract-all queue drops target when `start` fails after pop | **verified_fixed** |
| R1-P5 | low | Home Recent → Open ignores `dialog_open` debounce | **verified_fixed** |
| R1-P6 | low | `runner_busy` dead clause; queue not treated as busy | **verified_fixed** |
| R1-P7 | low | `last_parent_dir` never used for rfd initial directory | **verified_fixed** |

**Open medium+ from R1:** none.

---

## R1 Evidence (per finding)

### R1-P1 — verified_fixed

**Was:** `open_matter` always called `Matter::open` (temp wipe); Home open/recent had no busy guard.

**Now:**

1. `job_may_be_writing()` blocks create/open when `runner.is_busy()` or progress `state == "running"`.
2. UI disables Create / Open folder / Recent Open while writing; banner explains cancel/wait.
3. `open_matter(root, cleanup_temp)` uses `Matter::open` only when cleanup is requested; refresh still uses `open_for_read` only.

```86:89:crates/dedupe-desk/src/app.rs
    fn job_may_be_writing(&self) -> bool {
        self.runner.is_busy() || self.progress_rx.borrow().state == "running"
    }
```

```177:200:crates/dedupe-desk/src/app.rs
    fn open_matter_at(&mut self, path: PathBuf) {
        if self.job_may_be_writing() {
            self.error_msg = Some(
                "A job is still running. Cancel or wait before opening another matter \
                 (temp cleanup must not race extract)."
                    .into(),
            );
            return;
        }
        // ...
        match matter_ui::open_matter(&root, true) {
```

```17:26:crates/dedupe-desk/src/matter_ui.rs
/// When `cleanup_temp` is true, uses [`Matter::open`] (wipes orphaned
/// `workspace/temp/`). Only safe when **no** process-runner job is writing.
/// When false, uses [`Matter::open_for_read`] (no temp wipe).
pub fn open_matter(root: &Utf8Path, cleanup_temp: bool) -> Result<String, String> {
    let matter = if cleanup_temp {
        Matter::open(root).map_err(|e| e.to_string())?
    } else {
        Matter::open_for_read(root).map_err(|e| e.to_string())?
    };
```

Create path also gated (`create_matter_at` + Home Create enabled flags). Idle open still uses full `Matter::open` for orphan temp cleanup — correct and matches required_fix option (1).

---

### R1-P2 — verified_fixed

**Was:** Busy copy said “Cancel or wait”; Resume only for watch `paused`/`failed`; durable Running had no path.

**Now:**

1. `format_runner_error(Busy)` guides **Resume** for active or leftover Running.
2. `note_start_error` seeds `last_job_id` from `Busy { job_id }` and sets status to click Resume.
3. `can_resume()` allows durable `running`/`paused`/`failed` from jobs list; allows `last_job_id`; blocks spam Resume only when **live** in-process `running` + `runner.is_busy()`.
4. `resume_active` falls back to first durable job row when watch/last id empty.
5. Workspace Resume hover: “leftover Running after crash”.
6. Unit test `busy_error_mentions_resume`.

```66:75:crates/dedupe-desk/src/params.rs
pub fn format_runner_error(err: &process_runner::RunnerError) -> String {
    match err {
        process_runner::RunnerError::Busy { job_id } => {
            format!(
                "A job is already active or left Running (job {job_id}). \
                 Use Resume for that job, or wait if it is still processing."
            )
        }
```

```236:244:crates/dedupe-desk/src/app.rs
    fn note_start_error(&mut self, e: process_runner::RunnerError) {
        if let process_runner::RunnerError::Busy { ref job_id } = e {
            self.last_job_id = Some(job_id.clone());
            self.status_msg = Some(format!(
                "Busy on job {job_id}. Click Resume to continue a leftover/active job."
            ));
        }
        self.error_msg = Some(format_runner_error(&e));
    }
```

```366:383:crates/dedupe-desk/src/app.rs
    pub(crate) fn can_resume(&self) -> bool {
        let snap = self.progress_rx.borrow().clone();
        if snap.state == "running" && self.runner.is_busy() {
            return false;
        }
        // ...
        if self.last_job_id.is_some() {
            return true;
        }
        self.snapshot
            .jobs
            .iter()
            .any(|j| matches!(j.state.as_str(), "running" | "paused" | "failed"))
    }
```

---

### R1-P3 — verified_fixed

**Was:** soft path only if error contained `"busy"`; rusqlite often `"database is locked"`.

**Now:** `is_transient_sqlite_lock` matches `busy` / `locked` / `database is locked` / `sqlite_busy`; soft status + 25ms one-shot retry; unit tests cover locked vs hard error.

```78:85:crates/dedupe-desk/src/params.rs
pub fn is_transient_sqlite_lock(err: &str) -> bool {
    let e = err.to_lowercase();
    e.contains("busy")
        || e.contains("locked")
        || e.contains("database is locked")
        || e.contains("sqlite_busy")
}
```

```119:130:crates/dedupe-desk/src/app.rs
            Err(e) => {
                if is_transient_sqlite_lock(&e) {
                    self.status_msg = Some("Matter busy; will retry refresh…".into());
                    std::thread::sleep(Duration::from_millis(25));
                    if let Ok(snap) = matter_ui::refresh_snapshot(&root) {
                        // ... apply snap, clear soft status
                    }
                } else {
                    self.error_msg = Some(format!("Refresh failed: {e}"));
                }
            }
```

---

### R1-P4 — verified_fixed

**Was:** `pop_front` before `start`; failed start dropped the target.

**Now:** peek front → `start_extract_one` → `pop_front` only on `true`; on failure leave queue + status “Extract queue paused…”.

```299:314:crates/dedupe-desk/src/app.rs
    fn pump_extract_queue(&mut self) {
        if self.runner.is_busy() || self.progress_rx.borrow().state == "running" {
            return;
        }
        // Peek then pop only after successful start (R1-P4).
        let Some(next) = self.extract_queue.front().cloned() else {
            return;
        };
        if self.start_extract_one(next.source_id, next.pst_item_id) {
            let _ = self.extract_queue.pop_front();
        } else {
            self.status_msg = Some(
                "Extract queue paused (start failed). Resume the busy job or try again.".into(),
            );
        }
    }
```

`start_extract_one` returns `bool`. Meets R1 required_fix (pop only after success + pause with explicit status).

---

### R1-P5 — verified_fixed

**Was:** Recent Open ignored `dialog.is_open()`.

**Now:**

```477:485:crates/dedupe-desk/src/app.rs
            let can_open_recent = !self.dialog.is_open() && !self.job_may_be_writing();
            for path in recent {
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(can_open_recent, egui::Button::new("Open"))
                        .clicked()
                    {
                        self.open_matter_at(PathBuf::from(&path));
                    }
```

Also gated by job writing (bonus beyond R1).

---

### R1-P6 — verified_fixed

**Was:** third clause redundant with `state == "running"`; non-empty queue alone not busy.

**Now:**

```80:84:crates/dedupe-desk/src/app.rs
    pub(crate) fn runner_busy(&self) -> bool {
        self.runner.is_busy()
            || self.progress_rx.borrow().state == "running"
            || !self.extract_queue.is_empty()
    }
```

Queue draining disables Add/Extract as intended. See residual note R2-N1 (low).

---

### R1-P7 — verified_fixed

**Was:** `last_parent_dir` written never read for picker.

**Now:** Create matter passes `settings.last_parent_dir` into `dialog.spawn`; `DialogState::spawn` calls `rfd::FileDialog::set_directory` when present.

```445:447:crates/dedupe-desk/src/app.rs
                let initial = self.settings.last_parent_dir.as_ref().map(PathBuf::from);
                self.dialog.spawn(DialogKind::CreateParentFolder, initial);
```

```41:54:crates/dedupe-desk/src/dialogs.rs
    pub fn spawn(&mut self, kind: DialogKind, initial_dir: Option<PathBuf>) {
        // ...
                if let Some(dir) = initial_dir {
                    dlg = dlg.set_directory(dir);
                }
```

---

## New Findings

| ID | Severity | Title | Status |
|---|---|---|---|
| R2-N1 | low | Paused extract queue + `runner_busy` can block “try again” without Resume/reopen | open (non-blocking) |
| R2-N2 | low | Soft-lock retry uses `thread::sleep(25ms)` on UI thread | open (non-blocking) |
| R2-N3 | low | `can_resume` true whenever `last_job_id` is set (incl. post-success) | open (non-blocking) |

### R2-N1 (low)

- **description:** After Extract-all `start` fails, the queue is paused (correct for R1-P4) and `runner_busy()` stays true while the queue is non-empty (R1-P6). Workspace **Extract all / selected / Add** remain disabled. Status text says “try again,” but re-queue buttons are busy-disabled. **Busy** path recovers via Resume + later pump on job end. Non-Busy start failures (e.g. `InvalidParams`, matter open) have no Cancel (Cancel only when watch `running`) and no clear-queue control; operator must Home → reopen matter (`set_matter` clears queue) or restart. Rare; product copy slightly overstates “try again.”
- **files:** `crates/dedupe-desk/src/app.rs` (`pump_extract_queue`, `runner_busy`, `start_extract_all`); `workspace.rs` busy-gated buttons
- **suggested polish:** On non-Busy start failure clear or offer “Clear queue”; enable Extract all when queue paused and runner idle; or auto-clear queue on permanent errors.
- **blocks CLEAN?** No (low only).

### R2-N2 (low)

- **description:** `refresh_matter_lists` sleeps 25ms on the UI thread before one-shot retry under soft lock. Acceptable for rare contention; slightly freezes the frame vs scheduling retry next frame / off-thread.
- **files:** `crates/dedupe-desk/src/app.rs` (~124)
- **blocks CLEAN?** No.

### R2-N3 (low)

- **description:** `if self.last_job_id.is_some() { return true; }` keeps Resume enabled after successful jobs until a later state replaces id. Click may yield runner error (honest). Does not block durable Busy fix.
- **files:** `crates/dedupe-desk/src/app.rs` (`can_resume`)
- **suggested polish:** Enable Resume only for non-terminal / durable-running / last failed id.
- **blocks CLEAN?** No.

No medium or high new findings. No regressions of R1-P1–P7 observed.

---

## Completeness / Wiring Sweep (R2)

| Check | Result |
|---|---|
| TODO/FIXME/todo!/unimplemented! in desk | None |
| Stub Reduce/Review/Produce | Intentional “Coming soon” |
| Free-run `request_repaint()` | None — only `request_repaint_after(100ms)` |
| Stage APIs on UI (`ingest_path` / `extract_pst_*`) | None — ProcessRunner only |
| Refresh | `open_for_read` + list/count APIs |
| Exit | `on_exit` + `Drop` → `runner.shutdown()` |
| Handlers | Ingest + ExtractPst registered in `DeskApp::new` |
| Docs | Root README / ARCHITECTURE / crate README still describe Desk contracts |

---

## Verification Evidence

| Item | Observed now |
|---|---|
| Static re-verify of all R1 IDs in current desk sources | Yes |
| Regression / residual sweep (queue, resume, dialog, soft lock) | Yes — 3 low notes |
| `cargo test -p dedupe-desk` / workspace / clippy / fmt | **Not run** |
| `ledgerful verify` | **Not run** |
| Manual GUI smoke | **Not run** |

---

## Completion Decision

**CLEAN** — all R1 findings **verified_fixed** with code evidence; no open medium+ issues.

Optional polish: R2-N1–N3 (low). Orchestrator should still run workspace gates + manual smoke and write canonical `review.md` / finalize conductor; this R2 alone does not execute DoD-9/10 gates.
