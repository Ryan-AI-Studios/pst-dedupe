# 0020 â€” Desk shell UX â€” Plan

Phased checklist. Map phases to DoD items in `spec.md` Â§7. Execute in `C:\dev\dedupe`.

> **Ledger:**  
> `ledgerful ledger start 0020-deskshellux --category FEATURE --message "Dedupe Desk shell: matter, sources, process progress"`  
> Commit in Finalize.

---

## Phase 0 â€” Preconditions â†’ DoD-9 baseline

- [x] Confirm **0019** Completed: `../0019-ProcessJobRunner/review.md`, `crates/process-runner/README.md`
- [x] Confirm API: `start` / `resume` / `cancel` / `watch_progress` / `shutdown`; kinds `ingest` / `extract_pst`
- [x] Read plan-of-record Â§4.5 steps 1â€“3, Â§4.6 UI vs blocking
- [x] GUI stack: workspace **eframe 0.34**, **rfd 0.17** â€” **do not bump** to 0.35 unless MSRV/CI ready
- [x] **Verify WAL:** open a matter, `PRAGMA journal_mode` â†’ `wal` (matter-core `configure_connection`); note for concurrent UI read
- [x] Note rfd: off UI thread + **dialog_open debounce**
- [x] Note repaint: **`request_repaint_after(100ms)`** while job Running â€” never free-run `request_repaint()`
- [x] `cargo test -p process-runner` / `matter-core` green
- [x] **Crate choice: Option B** â€” new `crates/dedupe-desk`; leave `pst-dedup-gui` for legacy regression

## Phase 1 â€” Design lock â†’ DoD-1/5 prep

- [x] Freeze product window title: **Dedupe Desk**
- [x] Freeze binary package: **`dedupe-desk`**
- [x] Freeze nav: Home (matters) | Workspace (sources/process/jobs) | stubs for Reduce/Review/Produce
- [x] Freeze process flow: Add source â†’ Ingest â†’ list PSTs â†’ Extract (selected/all sequential)
- [x] Freeze UIâ†”runner: only process-runner for heavy work
- [x] Freeze rfd: background thread + `dialog_open` disables all pickers until result
- [x] Freeze repaint: `request_repaint_after(Duration::from_millis(100))` while job active
- [x] Freeze UI read: `Matter::open_for_read` for lists/stats during jobs
- [x] Freeze settings: last matter path
- [x] Sketch state machine: `Home` / `Workspace` / error overlays

## Phase 2 â€” Scaffold `dedupe-desk` â†’ DoD-1

- [x] `cargo new --bin crates/dedupe-desk` (or lib+bin as preferred)
- [x] Workspace member in root `Cargo.toml`
- [x] Deps: `process-runner` (default features), `matter-core`, `eframe` 0.34, `rfd` 0.17, `camino`, `serde_json`
- [x] Own `ProcessRunner` for app lifetime; register Ingest + ExtractPst handlers
- [x] On exit: `runner.shutdown()` in `on_exit` / Drop
- [x] `cargo run -p dedupe-desk` launches empty shell
- [x] Do **not** gut `pst-dedup-gui` in this phase (keep building)

## Phase 3 â€” Matter create/open â†’ DoD-2

- [x] Home UI: Create / Open / Recent
- [x] Off-thread folder pickers + `dialog_open` gate
- [x] `Matter::create` / `Matter::open` with error panel
- [x] Persist last path
- [x] Enter Workspace with `matter_root` in app state

## Phase 4 â€” Sources + jobs UI â†’ DoD-3, DoD-4, DoD-6

- [x] Add Folder / ZIP / PST buttons (off-thread rfd + debounce)
- [x] `start("ingest", {path})`; handle `Busy` / errors
- [x] Progress panel: each frame `watch_progress().borrow()`
- [x] While job active: **`ctx.request_repaint_after(100ms)`** only (not unconditional repaint)
- [x] Cancel / Resume buttons
- [x] Refresh discovered PSTs / jobs via **`open_for_read`** (WAL concurrent with worker)
- [x] Extract selected / extract all (sequential; disable while Busy)
- [x] Jobs table from `list_jobs`
- [x] Simple counts panel
- [x] Smoke: UI refresh during running job does not panic on SQLITE_BUSY

## Phase 5 â€” Polish + safety â†’ DoD-5, DoD-7

- [x] Code review: no stage APIs on UI thread
- [x] Code review: no free-run `request_repaint()` while Running
- [x] Code review: all pickers honor `dialog_open`
- [x] Exit path joins runner
- [x] Empty states / error copy

## Phase 6 â€” Docs + smoke â†’ DoD-8

- [x] Root README: `dedupe-desk` build/run; note legacy `pst-dedup-gui`
- [x] `crates/dedupe-desk/README.md`: UI vs worker, repaint throttle, dialog debounce, WAL
- [x] ARCHITECTURE.md Desk shell note
- [x] Manual smoke steps in `review.md` draft
- [x] Release build smoke on Windows

## Phase 7 â€” Verification â†’ DoD-9

- [x] Unit tests for pure helpers / state if extracted
- [x] `cargo test -p process-runner` / `matter-core` regression
- [x] `cargo build --release -p dedupe-desk`
- [x] `cargo build --release -p pst-dedup-gui` (legacy still builds)
- [x] `cargo fmt --all --check`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo test --workspace`
- [x] `ledgerful verify` (**required**)

## Phase 8 â€” Finalize â†’ DoD-10

- [x] Write `review.md` (Option B, repaint 100ms, dialog debounce, WAL verify, smoke evidence)
- [x] Update `../conductor.md`: **0020** â†’ **Completed**
- [x] Update `../sequencing.md`
- [x] Commit ledger TX
- [x] Handoff: **0021** â€œRun dedupeâ€ later; **0026** review nav becomes real

---

## Suggested file map (Option B)

```
crates/dedupe-desk/
  Cargo.toml
  README.md
  src/
    main.rs
    app.rs              # DeskApp state machine
    matter_ui.rs        # create/open/recent
    workspace.rs        # sources, process, jobs
    progress_ui.rs      # watch + throttled repaint
    dialogs.rs          # off-thread rfd + dialog_open
```

Legacy (unchanged for regression):

```
crates/pst-dedup-gui/   # FileSelect â†’ Scanning â†’ Results
```

---

## UI thread checklist (DoD-5 / 3.6.1)

- [x] No `ingest_path` / `extract_pst_*` / long Matter write on UI thread  
- [x] No sync `rfd` on UI thread  
- [x] `dialog_open` disables pickers until dialog returns  
- [x] Progress via `watch_progress` only  
- [x] **`request_repaint_after(100ms)`** while job Running â€” **not** every-frame `request_repaint()`  
- [x] UI lists use `open_for_read` (WAL)  
- [x] `shutdown` on exit  

---

## Default UX copy

| State | Message |
|---|---|
| No matter | â€œCreate or open a matter to begin.â€ |
| Job Running | â€œProcessingâ€¦ (cancel anytime)â€ |
| Busy | â€œA job is already running. Cancel or wait.â€ |
| Paused | â€œJob paused. Resume when ready.â€ |
| Failed | Show `error_summary` from snapshot |
| Dialog open | Buttons disabled (no multi-dialog) |

---

## Handoff notes

- **0019** owns execution; Desk is a thin reactive shell.  
- **Primary binary:** `dedupe-desk.exe`; keep `pst-dedup-gui` until product retires it.  
- **Repaint throttle** is a performance DoD, not polish.  
- **WAL** is already in matter-core â€” verify, donâ€™t reimplement.  
- **0021** registers dedupe handler + one more button.  
- **0026** replaces Review stub.  
- Stay on **eframe 0.34** unless a separate track bumps MSRV.  
- Single-exe / no-daemon / offline default unchanged.  
