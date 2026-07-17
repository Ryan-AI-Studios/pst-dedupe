# 0020 — Desk shell UX (matter + sources + process)

- **Track ID:** 0020-DeskShellUx
- **Execution repo:** `C:\dev\dedupe`
- **Governance:** this directory in `C:\dev\dedupe\conductor\`
- **Plan-of-record reference:** `C:\dev\Dedupe-plan.md` → §4.5 (matter lifecycle steps 1–3), §4.6 (UI vs blocking), Series A / **020**, §7 P0 (single-exe, process progress)
- **Cross-repo contract:** n/a
- **Status:** Completed
- **Depends on:** **0019-ProcessJobRunner** (Completed — `process-runner` with watch + Option C handlers)

---

## 1. Objective

Ship a **single-exe Windows desktop shell** for **Dedupe Desk** that an operator can use to:

1. **Create / open** a matter on disk  
2. **Add sources** (Purview export folder, ZIP, or PST)  
3. **Run process jobs** (ingest → list discovered PSTs → extract) with **live progress** and **cancel / resume**  
4. See basic **sources, jobs, and counts** — without servers, daemons, or cloud  

This is the user-visible front door for Series A foundation work. It is **not** full review UI (0026), dedupe controls (0021), or production export (0040).

---

## 2. Context (read before starting)

### 2.1 Plan-of-record

User-visible lifecycle (§4.5):

```
1. Create Matter
2. Add Sources (Purview export folder / PST / ZIP)
3. Process (progress UI; early stats stream in)
4. Reduce … 5. Review … 6. Produce … 7. Close   ← later tracks
```

**0020 delivers steps 1–3** in a cohesive shell. Reduce/review/produce only as **placeholders / disabled nav** if useful for IA, not full features.

### 2.2 What 0019 delivered (consume — do not reimplement)

From `../0019-ProcessJobRunner/review.md` + `crates/process-runner/README.md`:

| Surface | UI contract |
|---|---|
| `ProcessRunner::start(matter_root, kind, params)` | Create job + run on **matter worker** |
| `resume` / `cancel` | Cooperative cancel → stage **Paused** |
| `watch_progress()` | **`tokio::sync::watch`** latest `JobProgressSnapshot` — **poll on UI frame** |
| `subscribe_events` | Optional full stream (not required for progress bar) |
| Handlers | `ingest`, `extract_pst` (default features) |
| Drop/shutdown | cancel + **join** worker — call on app exit |
| Single-flight | Second start → `Busy` if job Running |

**Hard rule (0019):** UI thread may only call `start` / `resume` / `cancel` / `watch_progress` / `shutdown`.  
**Never** call `ingest_path`, `extract_pst_*`, or open `Matter` for long writes on the UI thread.

### 2.3 Existing GUI + crate choice

| Asset | Role |
|---|---|
| `crates/pst-dedup-gui` | Legacy **PST scan/dedup** wizard (`FileSelect` → `Scanning` → `Results`); `eframe` **0.34**, `rfd` **0.17**, ad-hoc worker — **not** process-runner |
| Binary today | `pst-dedup-gui.exe` |

**Decision for 0020 (required default — freeze in plan Phase 1):**

| Option | Recommendation |
|---|---|
| **A. Evolve `pst-dedup-gui` into Desk** | Allowed only if clearly cheaper; risk of spaghetti fighting the 3-step wizard state machine |
| **B. New `crates/dedupe-desk` binary** | **Recommended (default).** Clean `main.rs` + Matter/Workspace lifecycle; leave `pst-dedup-gui` intact for **legacy engine regression** until Desk reaches scan parity or product decides to retire it |

**P0 product binary:** `dedupe-desk.exe` (Option B). Do not gut the legacy wizard unless Option A is explicitly re-chosen with a short spike proving less rework.

### 2.4 GUI stack research (2026-07 — live)

| Fact | Implication for 0020 |
|---|---|
| **eframe 0.35.0** is current on crates.io (2026-06-25); workspace pins **eframe 0.34** | **P0: stay on 0.34** unless rustc MSRV is already ≥ what 0.35 needs. Plan historically noted **0.35 MSRV ~1.92** — do **not** bump as a drive-by; optional Phase “compat” only if CI/toolchain already supports it. |
| eframe is the standard desktop host for egui (native + optional wasm) | Desktop-only for Desk P0 (Windows primary; no wasm DoD). |
| **rfd 0.17** sync `pick_folder` / `pick_file` on the **UI thread freezes/hangs** egui (egui discussion #5621) | **Required:** dialogs on a **background thread**; apply result next frame |
| Immediate-mode UI | Rebuild panels each frame from matter snapshot + watch borrow |
| Unconditional `request_repaint()` while job runs | **Forbidden** — burns a core at 60–144 Hz (see §3.6) |
| SQLite concurrent UI read + worker write | Requires **WAL** (already set in matter-core — verify §2.6) |

### 2.5 matter-core concurrency (WAL — required for live refresh)

Track **0015** already configures connections with:

```sql
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
```

(`schema::configure_connection`, used by `Matter::open` / `create` / `open_for_read`.)

**Without WAL**, a UI `open_for_read` while the process-runner holds a write transaction can fail with **`SQLITE_BUSY`** and break live jobs/stats refresh.

**0020 requirements:**

1. **Do not** open matter DBs with a code path that skips `configure_connection`.
2. Prefer **`Matter::open_for_read`** for UI list/refresh (no `workspace/temp` wipe race with extract).
3. Phase 0: **verify** WAL is on for a live matter (`PRAGMA journal_mode` → `wal`) and that a short concurrent read during an ingest/extract job does not hard-fail the UI (retry/backoff once if transient busy is still possible under load).
4. If WAL were ever missing, that is a **matter-core bugfix** in this track — do not “fix” by blocking UI until jobs finish.

### 2.6 Desktop / product rules

- Single-exe; no Postgres/Redis/Docker.
- AI off / not exposed in P0 shell.
- Never mutate source PST/Purview trees.
- Prefer honest errors in UI (toasts / error panel), not silent failure.

---

## 3. In scope

### 3.1 Application shell

1. **Window title / product name:** “Dedupe Desk” (subtitle may mention pst-dedupe foundation).
2. **Navigation (minimal):**
   - **Home / Matters** — create, open recent, open folder
   - **Matter workspace** — sources, process, jobs, simple stats
   - Optional stub sections: Reduce / Review / Produce (disabled or “Coming soon”) so IA matches plan §4.5 without implementing them
3. **Matter paths:** user-chosen directory (e.g. `Matters/<name>/`); use `Matter::create` / `Matter::open` via **short** UI-thread opens **only for read-only listing** where safe (`open_for_read` preferred for refresh), or open on worker for writes — document chosen pattern.
4. **App exit:** `ProcessRunner::shutdown()` (or drop runner) so worker joins.

### 3.2 Create / open matter

| Action | Behavior |
|---|---|
| Create | Prompt name + parent directory (rfd **folder** pick off-thread); `Matter::create(root, name)`; store last-opened path in local settings (egui memory / simple json under user config — optional but recommended) |
| Open | Folder picker → `Matter::open`; validate `matter.db` exists |
| Recent | List last N paths if settings exist |

Errors: path exists / not a matter / IO — show in UI.

### 3.3 Add sources

| Source type | UI | Backend |
|---|---|---|
| Folder / Purview package | “Add folder…” | `runner.start(..., "ingest", {"path":...})` |
| ZIP file | “Add ZIP…” | same ingest kind |
| Single PST | “Add PST…” | ingest **or** `extract_pst` with path form — **prefer ingest** for package detect consistency, then extract; or direct `extract_pst` with `{path}` params |

After successful ingest, refresh **sources** and **discovered PSTs** (`list_discovered_psts` via short read connection or job completion handler).

**Do not** expand ZIP or parse PST on the UI thread.

#### 3.3.1 Native file dialogs — off-thread + **debounce**

1. Spawn `rfd` on a **background thread** (never sync pick on UI thread).
2. Deliver path (or cancel) via channel / `Arc<Mutex<Option<…>>>` and apply on a later frame.
3. **Debounce multi-click:** UI state `dialog_open: bool` (or enum which dialog):
   - Set `true` when spawning the dialog thread.
   - **Disable** all Add/Open/Create picker buttons while `dialog_open`.
   - Set `false` when the channel returns a path **or** cancellation / error.
4. Prevents five overlapping Explorer windows from impatient clicks while the first dialog is still opening.

### 3.4 Process / jobs panel

1. **Start ingest** for a pending path (if not auto-started on add).  
2. **List discovered PSTs** (inventory items).  
3. **Extract selected PST** / **Extract all discovered** (queue: P0 may be sequential only — second start returns Busy; UI disables Start while Running or auto-chains after Succeeded).  
4. **Progress bar / labels** bound to `watch_progress().borrow()` each frame:
   - kind, stage, state, `completed_count`, message, error_summary  
5. **Cancel** button → `runner.cancel(job_id)`.  
6. **Resume** for Paused/Failed when applicable → `runner.resume(matter_root, job_id)`.  
7. **Jobs table:** `Matter::list_jobs` (read) — id, kind, state, timestamps.  
8. **Busy handling:** show clear message if `start` returns `Busy` (durable Running or active slot).

### 3.5 Status / stats (minimal)

Read-only snapshot after jobs (via `open_for_read` or post-job):

- Source count / kinds  
- Item counts (total, by status if cheap)  
- Last job result  

No Tantivy, no review list, no coding UI.

### 3.6 UI architecture constraints

```text
egui frame (UI thread)
  ├─ borrow watch snapshot → paint progress
  ├─ buttons → runner.start/cancel/resume only
  ├─ rfd → spawn thread → channel path back (dialog_open gate)
  └─ request_repaint_after(100ms) while job Running (not every frame)

process-runner matter-worker
  └─ Matter open → job → ingest/extract handlers
```

| Do | Don't |
|---|---|
| Poll `watch` each frame (cheap borrow) | Block UI on extract |
| Off-thread rfd + `dialog_open` debounce | Sync rfd on UI thread; multi-dialog spam |
| `shutdown` on exit | Detach runner / kill -9 without join |
| Show stage errors | Swallow `RunnerError` |
| Throttled repaint while Running | Unconditional `request_repaint()` every `update()` |

#### 3.6.1 Repaint policy — **no 144 Hz CPU burner**

While a job is **Running** / **Paused**-pending-UI, the progress bar must update without user input — but **must not** max out the display refresh rate.

| Approach | Status |
|---|---|
| `ctx.request_repaint()` every `update()` while Running | **Forbidden** — drives 60–144 FPS continuous redraw, burns a core, starves the matter worker on the same machine |
| `ctx.request_repaint_after(Duration::from_millis(100))` while job active | **Required** — ~**10 FPS** is smooth enough for progress; UI CPU ≈ idle |
| `request_repaint()` once when `watch` snapshot **changes** (if `changed()` / version available) | **Allowed** as supplement; still cap with `request_repaint_after` if change storms |
| Idle (no job) | No forced repaint loop; egui default input-driven frames |

Document the 100 ms interval (or 50–200 ms range) in crate README; do not “optimize” by removing the throttle.

### 3.7 Settings / hygiene

- Matter root / last path persistence (simple).  
- Log level optional.  
- No AI keys, no cloud endpoints in P0.  
- About panel: version, offline notice.

### 3.8 Tests & verification

GUI is hard to automate headlessly. **Required mix:**

1. **Logic unit tests** for pure helpers (path validation, nav state machine, params JSON builders) if extracted from UI.  
2. **Integration without display:** optional `eframe` headless not required; prefer testing “controller” layer that calls `ProcessRunner` with fixture paths (can reuse process-runner patterns).  
3. **Manual smoke script** (PowerShell) documented in README:
   - build release GUI  
   - create matter under `output/desk-smoke/`  
   - add `fixtures/` PST or purview sample  
   - run ingest + extract; cancel once; resume  
4. Workspace gate + **`ledgerful verify`**.

### 3.9 Docs

- Update root `README.md`: Desk binary name/path, screenshots optional.  
- Crate README for Desk/GUI: UI thread rules, rfd off-thread, process-runner wiring.  
- `ARCHITECTURE.md` shell section.  
- `review.md` on completion.

### 3.10 Optional (not DoD)

- Drag-and-drop paths onto window.  
- System dark/light follow.  
- Multi-window.  
- egui 0.35 upgrade.  
- Full review list / dedupe buttons.  
- Embed CLI log panel.

---

## 4. Out of scope (do NOT do here)

| Deferred | Work |
|---|---|
| **0021** | Matter dedupe job UI + handler beyond “coming soon” |
| **0026–0027** | Review list, coding, batch |
| **0029** | FTS search UI |
| **0040** | Production export UI |
| **0019** (done) | Runner internals (consume only) |
| — | Web/wasm Desk, multi-user, AI features |
| — | Drive-by egui 0.35 + MSRV bump unless isolated and justified |
| — | Mutating source evidence |

---

## 5. Preconditions & dependencies

- **P1 (blocking):** **0019** Completed — `process-runner` with `IngestHandler` / `ExtractPstHandler`, watch, Option C.  
- **P2:** **0015–0018** foundation (matter, ingest, extract) available.  
- **P3:** Workspace builds on Windows.  
- *Verified research / repo snapshot:*
  - Workspace `eframe = "0.34"`, `rfd = "0.17"`
  - crates.io latest eframe **0.35.0** (2026-06-25) — **not required** for this track  
  - `process-runner` public API as in crate README  
  - Legacy GUI still scan-centric  

---

## 6. Risks

| Risk | Mitigation |
|---|---|
| UI freeze from extract | Only process-runner on worker; no stage calls on UI |
| **144 Hz repaint / CPU burner** | **`request_repaint_after(100ms)` only** while job active (§3.6.1) |
| rfd freezes egui | Dialogs on dedicated thread |
| **Multi dialog spam** | **`dialog_open` debounce**; disable Add/Open while open |
| Double Matter open / **SQLITE_BUSY** | **WAL** (0015 already); `open_for_read` for UI; verify concurrent read |
| Scope creep into review/dedupe | Hard out-of-scope; stub nav only |
| egui 0.35 MSRV break | Stay on 0.34 unless toolchain ready |
| Busy / crash leftover Running job | UI surfaces Busy + guidance to resume or mark failed |
| App exit mid-job | Runner Drop joins; document “wait on close” |
| Legacy wizard spaghetti | **Option B `dedupe-desk`**; keep `pst-dedup-gui` for regression |

---

## 7. Definition of Done

Complete only when ALL hold:

- [x] **DoD-1 — Single-exe Desk:** Primary release binary **`dedupe-desk`** (Option B default) builds and launches on Windows without servers; legacy `pst-dedup-gui` still builds if kept.  
- [x] **DoD-2 — Matter UX:** Create + open matter from UI; errors visible.  
- [x] **DoD-3 — Sources + process:** Add folder/ZIP/PST; start **ingest** and **extract_pst** via `ProcessRunner` only.  
- [x] **DoD-4 — Progress + cancel:** Live progress from **watch**; Cancel → Paused; Resume works for a demo path; repaint **throttled** (`request_repaint_after` ~100 ms, not free-run).  
- [x] **DoD-5 — UI thread safety:** No stage extract/ingest on UI thread; rfd off-thread + **dialog debounce**.  
- [x] **DoD-6 — Concurrent refresh:** UI read path uses `open_for_read` / WAL-safe open; live jobs list or stats do not hard-fail with unhandled `SQLITE_BUSY` during a running job (smoke or test).  
- [x] **DoD-7 — Shutdown:** App exit shuts down runner (join).  
- [x] **DoD-8 — Docs + smoke:** README + manual smoke steps; ARCHITECTURE/README updated.  
- [x] **DoD-9 — Workspace gate:** fmt, clippy `-D warnings`, tests, **`ledgerful verify`**.  
- [x] **DoD-10 — Recorded:** `review.md`; `../conductor.md` → **Completed**; ledger TX (`FEATURE`).

---

## 8. Verification commands (reference)

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --release -p dedupe-desk
cargo build --release -p pst-dedup-gui   # legacy still builds
ledgerful verify

# Manual smoke (document exact clicks in review.md):
# .\target\release\dedupe-desk.exe
```

---

## 9. Acceptance narrative

Counsel/operator can:

1. Launch **`dedupe-desk.exe`** (single product shell).  
2. Create a matter under a chosen folder.  
3. Add a Purview-like folder or fixture PST/ZIP (one dialog at a time; buttons disabled while picker open).  
4. Watch ingest progress at ~10 UI FPS without pegging a CPU core; cancel and resume if needed.  
5. See discovered PSTs; run extract; jobs/stats refresh while the worker writes (WAL).  
6. Close the app cleanly (worker joins).  
7. Re-open the same matter later and see sources/jobs still present.  
8. Still run **`pst-dedup-gui`** for legacy quick scan if needed.  

They still need later tracks for full dedupe-as-job UI, review coding, and production export — but the **foundation process path is human-operable**.
