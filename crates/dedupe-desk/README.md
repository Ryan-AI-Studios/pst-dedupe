# dedupe-desk

**Dedupe Desk** — single-exe Windows shell for matter create/open, source ingest,
PST extract, and matter-level **tiered dedupe** with live progress (tracks **0020**, **0021**).

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
7. **Run dedupe** — tiered Message-ID → logical_hash → family attach policy
8. **Run threading** — Message-ID graph → subject → ConversationIndex → family inherit
9. **Run near-dup** — MinHash shingles + LSH clusters (pivot/member; not exact suppress)
10. Confirm jobs table + Counts (unique/duplicate) update; journal_mode shows `wal`
11. Close the app (worker joins; window may wait briefly if a job is finishing)

### Run dedupe

Workspace **Run dedupe** starts `kind=dedupe` with default params
(`use_message_id` / `use_logical_hash` / `suppress_children_with_parent` /
`reset` / `batch_size=500`). Uses the same progress / cancel / resume path as
ingest and extract. Identity is desk MID + `logical_hash` — not CLI content-hash.

### Run threading

Workspace **Run threading** starts `kind=thread` with default params
(`use_headers` / `use_subject_fallback` / `use_conversation_index` / `reset` /
`batch_size=500` / `family_inherit`). Reuses progress / cancel / resume.
Matters extracted before 0022 need **re-extract** to populate reply headers
(re-extract refreshes the four header columns on existing message paths).

### Run near-dup

Workspace **Run near-dup** starts `kind=neardup` with default params
(`minhash_shingle_v1`: k=5, cjk_char_n=2, H=128, 16×8 bands, threshold 0.80,
`skip_exact_duplicates`, `min_chars=80`, `reset`, `batch_size=200`). Reuses
progress / cancel / resume. Near-dup groups are **flag-only** — Desk does not
auto-hide them as exact duplicates. See `crates/matter-neardup/README.md`.

### Run cull

Workspace **cull preset** dropdown + **Run cull** starts `kind=cull`:

| Selection | Params shape |
|---|---|
| Built-in (`unique_only`, `unique_plus_family`, `noise_light`) | `{ "preset_name", "reset": true, "batch_size": 500 }` |
| User preset (from matter DB) | `{ "preset_id", "reset": true, "batch_size": 500 }` |

Built-ins that work out of the box are always listed. **User presets** appear
when present in the open matter’s `cull_presets` table (loaded on refresh).
Desk 0024 does not ship a full preset editor — create/update presets via
matter-core API (`upsert_cull_preset`) or future UI. `date_window` remains an
engine built-in but is **not** listed until bounds are filled — operators
supply `start`/`end` (offset-aware RFC3339) via JSON params or a user preset.
Flag-only: sets `cull_status` / reasons; never deletes items or CAS. Reuses
progress / cancel / resume. See `crates/matter-cull/README.md`. **0025 promote**
should prefer `cull_status=included` when present.

## Tests

```powershell
cargo test -p dedupe-desk
```

Pure helpers cover nav, params JSON, settings, WAL refresh snapshot, and dialog debounce. Full GUI interaction is manual (see above).
