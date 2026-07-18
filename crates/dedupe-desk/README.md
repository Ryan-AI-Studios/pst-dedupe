# dedupe-desk

**Dedupe Desk** — single-exe Windows shell for matter create/open, source ingest, and PST extract with live progress (track **0020**).

## Build / run

```powershell
cargo run -p dedupe-desk
# release:
cargo build --release -p dedupe-desk
.\target\release\dedupe-desk.exe
```

Legacy scan/dedup wizard remains at `pst-dedup-gui` (`pst-dedup-gui.exe`).

## Architecture (UI thread rules)

```text
egui UI thread                         process-runner matter-worker
─────────────────                      ────────────────────────────
start / resume / cancel     ──────►    open Matter, run handlers
watch_progress.borrow()                publish JobProgressSnapshot
open_for_read (lists/stats)            WAL writes
rfd on background thread               (never stage APIs on UI)
request_repaint_after(100ms)
  while job Running
shutdown / Drop               ──────►  cancel + join worker
```

### Hard rules

| Do | Don't |
|---|---|
| `ProcessRunner::start` / `cancel` / `resume` / `watch_progress` | Call `ingest_path` / `extract_pst_*` on the UI thread |
| `Matter::open_for_read` for live list refresh | `Matter::open` from a concurrent poller (wipes `workspace/temp`) |
| Off-thread `rfd` + `dialog_open` debounce | Sync file dialogs on the UI thread; multi-dialog spam |
| `request_repaint_after(100ms)` while Running | Free-run `request_repaint()` every frame (~144 Hz CPU burner) |
| `runner.shutdown()` on exit | Detach worker without join |

### Repaint policy

While a job is **Running**, the shell requests a repaint every **100 ms** (~10 FPS). That is enough for progress bars and does not peg a CPU core or starve the matter worker.

### Dialog debounce

`dialog_open` disables all Add/Open/Create pickers until the background `rfd` thread returns a path or cancel. Prevents stacked Explorer windows from double-clicks.

### WAL

matter-core configures `PRAGMA journal_mode=WAL`. The Counts panel shows the live journal mode. UI refresh uses `open_for_read` so concurrent worker writes do not race temp cleanup.

## Manual smoke (Windows)

1. `cargo build --release -p dedupe-desk`
2. Launch `.\target\release\dedupe-desk.exe`
3. **Create matter** under e.g. `output/desk-smoke/MyCase`
4. **Add PST** from `fixtures/` (or Add folder / ZIP)
5. Watch ingest progress; optionally **Cancel** then **Resume**
6. Select a discovered PST → **Extract selected** (or Extract all)
7. Confirm jobs table + counts update; journal_mode shows `wal`
8. Close the app (worker joins; window may wait briefly if a job is finishing)

## Tests

```powershell
cargo test -p dedupe-desk
```

Pure helpers cover nav, params JSON, settings, WAL refresh snapshot, and dialog debounce. Full GUI interaction is manual (see above).
