# 0020-DeskShellUx — Review

- **Track:** 0020-DeskShellUx
- **Status:** Completed — Codex **PASS WITH DEFERRED P3** (human GUI smoke D-0020-01)
- **Date:** 2026-07-17
- **Crate:** `crates/dedupe-desk` (+ `matter-core` list helpers)

## Summary

Primary product shell for Dedupe Desk:

| Area | Result |
|---|---|
| Binary | **`dedupe-desk`** (Option B); legacy `pst-dedup-gui` retained |
| Matter UX | Create / open / recent; create+open **off UI thread** (`matter_ops`) |
| Process | `ProcessRunner` only — `ingest` / `extract_pst` handlers registered |
| Progress | `watch_progress` borrow; **`request_repaint_after(100ms)`** while Running |
| Dialogs | Off-thread `rfd` + `dialog_open` debounce; `last_parent_dir` seed |
| Refresh | `Matter::open_for_read` + WAL; soft lock handling; concurrent reader test |
| Shutdown | `on_exit` + `Drop` → `runner.shutdown()` (join) |
| Busy | Durable/in-process Busy → Resume guidance; jobs table fallback |

## Public surfaces

### dedupe-desk
- Window title **Dedupe Desk**
- Nav: Home | Workspace | Reduce/Review/Produce stubs
- `ProcessRunner::start/cancel/resume/watch_progress/shutdown`
- Settings JSON under `%APPDATA%\dedupe-desk\settings.json`

### matter-core (additive)
- `list_sources`, `count_items`, `list_items_by_file_category`

## Verification

| Command | Result |
|---|---|
| `cargo fmt --all --check` | **PASS** |
| `cargo clippy --workspace --all-targets -- -D warnings` | **PASS** |
| `cargo test -p dedupe-desk` | **PASS** (17) |
| `cargo test --workspace` | **PASS** |
| `cargo build --release -p dedupe-desk` | **PASS** |
| `cargo build --release -p pst-dedup-gui` | **PASS** |
| `ledgerful verify` | **PASS** (fmt + clippy + test) |

### Manual smoke (documented; unit evidence substitutes interactive)

Documented in `crates/dedupe-desk/README.md`. Automated proof for non-GUI paths:

- Create/open/refresh + `journal_mode=wal` (`matter_ui` tests)
- Concurrent `open_for_read` while writer connected
- Off-thread create (`matter_ops`)
- Params/nav/Busy copy / dialog debounce unit tests
- Release binary present: `target\release\dedupe-desk.exe`

## Review loop

| Round | Verdict | Notes |
|---|---|---|
| Internal R1 | NEEDS_FIX | open-while-busy temp race; Busy resume; lock string; extract queue pop |
| Internal R2 | **CLEAN** | R1 all verified_fixed; low polish only |
| Codex R1 | **FAIL** | DoD-9/10 incomplete during review; Matter open on UI; concurrent evidence |
| Post-fix | — | Off-thread matter_ops; concurrent reader test; full gate green; DoD boxes + ledger commit |
| Codex R2 | **PASS WITH DEFERRED P3** | Behavioral P1/P2 fixed; only D-0020-01 human GUI smoke deferred |

## Design locks (confirmed)

- Option B `dedupe-desk`
- eframe **0.34** / rfd **0.17** (no drive-by 0.35)
- No stage ingest/extract on UI thread
- Repaint throttle 100 ms
- `open_for_read` for live lists
- Single-flight extract queue

## Deferred (`docs/deferred.md`)

| ID | Item |
|---|---|
| D-0020-01 | Full interactive GUI smoke by human (clicks + cancel/resume demo) |
| D-0020-02 | Drag-and-drop paths / dark mode / multi-window (spec optional) |
| D-0018-04 | Closed for Desk progress UI by this track |

## Unblocked

- **0021** MatterDedupeJob — register handler + one Workspace button
- **0026** Review nav stub becomes real later
