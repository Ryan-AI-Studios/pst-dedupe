# 0116 — Fold egui Process into the Tauri window

> Structure follows `templates/0000-Description/spec.md`. DoD is §7.
> Phase 0 is **closed** in this file. Do not re-open unique-export (0108–0109),
> matter-home overview math (**0110**), first-pass queue (**0111** / **0117**),
> three-pane coding (**0112** / **0118**), produce checklist UX (**0113** / **0119**),
> zpdf (**0114** / **0120**), TIFF/OPT factory (**0115** / **0121**), or Admin.
> Do not vendor `C:\dev\dedupe-frontend`. Do not mint a BCC-default track.

- **Track ID:** 0116-ProcessFold
- **Execution repo:** `C:\dev\Dedupe`
- **Governance:** this directory in `C:\dev\Dedupe\conductor\` (track registry: `../conductor.md`)
- **Plan-of-record reference:** Hermes Process workspace + `conductor/0110` stack lock. `C:\dev\Dedupe-plan.md` is **absent** (re-verified 2026-08-31); do **not** chase it at execute.
- **Cross-repo contract:** mock at `C:\dev\dedupe-frontend` is research only (pane density + copy, not tokens, not fake OST/MBOX/NSRL).
- **Status:** In Progress
- **Depends on:** **0110–0115 Completed** · Desk Process **0020** / runner **0019** / profiles **0043** · `matter-core` schema **v41**
- **Spec authored:** 2026-08-31 (placeholder → Ready)
- **Series:** O (Review chrome) — last spine track (0117–0121 are residual placeholders)
>
> **Closes / absorbs:** `D-0116-process-fold` (this track) and **`D-0113-long-job`** (chrome produce/QC leave blocking `join_worker`). Does **not** close D-0110-deny-unic, D-0117–D-0120, D-0121, D-0020-01 operator smoke, D-0016-05 7z, D-0024-01 NSRL RDS, D-0062-codesign.
> **HITL:** owner launches the **release** chrome EXE against a **synthetic** matter (temp folder + fixture PST/ZIP). INC* unique-pst is **not** a gate. Codesign is **D-0062-codesign**.
>
> **Last-PR fold-in (2026-08-31):** PRs **#122, #121, #120, #119**. Disposition in §2.8. Four **0115** image/QC Bugbot items **minted 0121**. Raster UI stays **0120**. Produce wizard stays **0119**. Window async stays **0118**. Queue stays **0117**.
>
> **Review fold-in (2026-08-31):** `opencode-review.md` + `agy-review.md`. Disposition in §2.10 and `foldin-note.md`.
>
> **Stack lock (inherit 0110–0115):** Tauri **2** + Leptos **0.8 CSR** on **stable** Rust. Plex / paper / cool chrome. Red = privilege / withhold / **blocker** / exception-risk only. No daemon. **This track adds `process-runner` to `dedupe-chrome`.** One pipeline. No WASM jobs.

---

## 1. Objective

Replace the **0110** `/matters/:id/process` stub with a live **Process workspace** on the same `dedupe-chrome` EXE: sources, locked built-in profile, jobs + exception quarantine, reconciliation with **Unaccounted-for: 0** after the golden path. Host **`process-runner`** (the 0019/0020/0045 worker) so ingest/extract/`profile_run` and chrome produce/QC get **cancel + progress** instead of blocking `join_worker`.

This advances **correctness**, not chrome polish: counsel can take a matter from empty → extracted → promoted **without launching `dedupe-desk`**, on the **same** jobs/checkpoints/`item_errors` Desk already uses. Exceptions quarantine their own items; they must **not** stall the rest of the collection. Unique-export is unchanged.

---

## 2. Context (read before starting)

### 2.1 Why this track, now

**0115 Completed** (PR **#121** / `19d0c1f`): image/OPT factory is in. Unique-export Series S is closed. Review / Produce chrome is real. The remaining counsel-facing hole is **Process still saying “stays in Dedupe Desk until 0116.”** Vault: do not start 0116 unless asked — this `/plan-track 116` **is** that ask (plan only).

**0113** parked multi-GB produce/QC cancel on this ID (`D-0113-long-job`). Absorb it here: do not leave Finalize as a frozen IPC call.

### 2.2 Live APIs (plan-time 2026-08-31, HEAD `c6fb70c`; re-verify at execute)

| Surface | Fact |
|---|---|
| `crates/matter-core/src/schema.rs` | `SCHEMA_VERSION == 41`. **No schema bump this track.** |
| `Matter::insert_source` / `list_sources` | **pub**. Source registration does **not** create items (`processed` stays 0 — 0110 lock). |
| `Matter::list_items_by_file_category("pst")` | Desk PST inventory (`PstRow`). Prefer this over `connection()` SQL. |
| `Matter::list_jobs` | Includes `parent_job_id` for `profile_run` children. |
| `Matter::item_errors_for_item` / `_for_source` / `_for_job` | **No** matter-wide list today. Chrome must not `connection()`. **This track adds** `Matter::list_item_errors_recent(&self, limit: u64)` (read-only `&Matter`, default cap **100**, newest `updated_at` first). |
| `load_case_overview` / `matter_overview` | Sources = `sources_total`; Processed = `top_level_items`; Exceptions = `errors.total`; Unreviewed = `unreviewed_count`. Reuse for the running-report pane. |
| `process-runner` | `ProcessRunner::start` / `resume` / `cancel` / `watch_progress` / `active_job` / `is_busy` / `shutdown`. Second `start` → `Busy { job_id }`. **Durable** single-flight: accept phase also refuses if **any** `JobState::Running` **row** exists (`runner.rs` accept), not only in-memory `active`. Option C: **runner creates the job row**. `resume` of the **same** id accepts `Paused`/`Failed`/`Pending`/`Running` (orphan crash recovery). `register_default_handlers` = Desk/CLI kind set (`produce`/`qc` are `cfg` feature-gated; chrome uses **default features**). `tokio` **sync only**. `JobProgressSnapshot` has `matter_id` (**not** `matter_root`). `JobSnapshot` (from `active_job`) has both. |
| Kinds (default features) | `ingest`, `extract_pst`, `dedupe`, `thread`, `neardup`, `cull`, `promote`, `produce`, `production_export`, `qc`, `gap`, `fts_index`, `office_extract`, `pdf_extract`, `ics_extract`, `teams_extract`, `ocr`, `transcribe`, `classify`, `entity_scan`, `people_graph`, `concept_cluster`, `sentiment`, `semantic_index`, `ai_suggest_codes`, `profile_run`, `workflow_run`. |
| Ingest params | `{ "path": "…" }` (Desk `ingest_params`). |
| Extract params | `{ "source_id", "pst_item_id" }` from inventory. |
| `profile_run` params | `{ "profile_id": "builtin:standard" }` (or other builtin). Canonical order: classify → office/pdf/ics extract → ocr → fts → dedupe → thread → neardup → cull → promote. **Ingest/extract_pst are not profile stages** — they run first. |
| `builtin:standard` (live) | classify + office/pdf/ics extract + dedupe + thread + cull `unique_only` + promote `auto`. **OCR / neardup / FTS off.** Cumulative `reset:false`. Description in `profile.rs` `builtin_profiles()`. |
| Other builtins | `with_ocr` = `standard_body(true)` (OCR on; **neardup stays false** — live `profile.rs` `stage(false, neardup_params())`, not tied to the OCR flag). `extract_only`, `reduce_only`. User profiles (`pfl_…`) exist in Desk; **chrome v1 does not edit/save them**. Derive checkmarks from `ProfileBody`, do not hardcode. |
| Chrome host today | Process route is `ProcessStub` (“Process stays in Dedupe Desk until 0116.”). Commands use `join_worker`. **No `process-runner` dep.** `tauri-plugin-dialog` already inited. Actor `"chrome"`. Encrypted → `encrypted`, no `open_*`. |
| Chrome produce | `produce_qc_run` / `produce_start` `create_job` + engine on `join_worker` (0113). `intended_produce_params` builds `ProduceParams`; `serde_json::to_value(...).unwrap_or_else(|_| json!({}))` at `produce.rs` (~747) is a **silent empty-JSON fallback — delete it this track** (return blocked, never `{}`). Runner `ProduceParams::from_json` / `QcParams::from_json` must round-trip chrome's struct. Privilege log co-export after produce. |
| Desk analog | `dedupe-desk` `app.runner: ProcessRunner` + `progress_rx` polled each frame. Add folder/ZIP/PST on a **background** dialog thread. Extract selected / all. Run profile / workflow. **Do not depend on `dedupe-desk`.** Duplicate param JSON in chrome (pure tests). |
| CI | `chrome-ui`: wasm32 + `trunk` + `cargo test -p dedupe-chrome`. Keep it. `ui/` stays workspace-excluded. Host may grow with process-runner default features (same as Desk). |
| MS-PST | **N/A this track** (extract still `extract-pst` on the worker). |

### 2.3 Mock + Hermes (research only; re-verified 2026-08-31)

`C:\dev\dedupe-frontend/frontend/src/pages/process.rs`: three panes (sources + locked profile | jobs table + exception master/detail | running report + reconciliation). Status bar: **“Processing is deterministic. No prediction, no coding, no privilege calls here.”** Unaccounted-for: **0**. Quarantine copy: file exceptions hold their own items; the rest of the collection goes to review.

**Steal:** three-pane density; jobs vs exceptions split; reconciliation kicker; status-bar sentence; Pause = cancel; “Open review-ready” → existing 0111 queue.

**Do not copy / do not fake:** coral tokens; **OST / MBOX** ingest; **NSRL 2026.08** DeNIST; password vault / “request from custodian”; 7z hydrate; invented per-source Dupes/NIST columns unless live `count_by_dedup_role` / cull flags exist; mock “threading is not inside processing” (live `builtin:standard` **includes** `thread`).

### 2.4 Plan-time crate pins (re-verify at execute)

| Crate | Plan-time | Rule |
|---|---|---|
| `tauri` | **2.11.5** (crates.io 2026-07-01) | `tauri = "2"`. Reject **3.x / pre-release**. |
| `leptos` | **0.8.x** CSR | Do not bump major. |
| `process-runner` | path crate, **default features** (match Desk/CLI handlers) | Host only. **Never** add to `dedupe-chrome/ui`. |
| `tokio` | via process-runner (`sync` only) | Do not enable a runtime for jobs. |
| Schema | **41** | No bump. |
| Rust | **stable** (CI) | No nightly. |

### 2.5 Tools / recall

Ran from `C:\dev\Dedupe`:

- `ai-brains preflight --summary` (inited; 4126 pinned).
- `ai-brains sync query` / recall: 0110–0115 chrome tracks forbade `process-runner`; Desk keeps the worker; 0113 parked long-job cancel here; no BCC.
- `ledgerful doctor --json` `readyForPublish: true`; `ledger status --compact` **0 pending / 0 unaudited drift** before this tx. Doctor: phantom-promote, impact-stale, sig-pin, sig-version — none block planning.
- Ledger tx for this planning pass: `616619cc-a814-4965-bab9-c5b591ab7283`.
- `scan --impact` after spec write (docs/conductor only expected **LOW**).

### 2.6 How this advances the north star

Counsel-facing ingest/extract must be **honest**: skip reasons land in `item_errors`, jobs checkpoint, cancel pauses instead of wedging the WebView, produce/QC can be cancelled. One pipeline (`process-runner` + `matter-core`). Not a second WASM ingest. Not unique-pst.

### 2.8 Last-PR Cursor comments (mandatory)

Last four merged product PRs: **#122** (docs 0115), **#121** (0115 TIFF/OPT), **#120** (docs 0114), **#119** (0114 raster).

| PR | Surface | Disposition |
|---|---|---|
| **#122** | docs registry | no review/issue/line comments |
| **#121** | ImageOptFactory | **Four valid Bugbot items** — not Process. **Minted 0121** (§2.8.1). Do not fold into 0116, 0119, or 0120. |
| **#120** | docs registry | none |
| **#119** | PdfRasterRedact UI | Already owned by **0120** (overlay coords / draw state / Burn counts). Do not steal. |

No BCC-default track. Next free ID after mint: **0122**.

#### 2.8.1 PR #121 → **0121-ImageOptQcResiduals** (Proposed placeholder)

| Bugbot | Severity | Fold |
|---|---|---|
| QC OPT check blocks resume | High | `opt_row_count_mismatch` Errors when pages exist but `IMAGE.opt` is not written yet → interrupted image produce cannot resume. `matter-qc` `rules.rs`. |
| QC scans every image volume | High | Image QC walks **every** `production_sets` row, not the current job; leftover/moved volumes Error `image_page_missing` and block a new Finalize. |
| JPEG path eligibility mismatches pages | Medium | `is_image_eligible_native` treats `.jpg`/`.png` as eligible when `sniff_kind` is Other and page count is 0 → native-only ship then fail-closed. |
| MIME wins over TIFF magic | Medium | `sniff_kind` returns JPEG/PNG from MIME before TIFF magic / `.tif` path. |

### 2.9 Product locks (do not invent at execute)

See §3.

### 2.10 Review fold-in (2026-08-31)

Sources: `opencode-review.md`, `agy-review.md`. Inputs not edited.

| Id | Sev | Disposition | Lock |
|---|---|---|---|
| opencode-M1 | Major | **Agree — fold** | Chrome `intended_produce_params` / QC params → `ProduceParams::from_json` / `QcParams::from_json` deep-equal. **Delete** `unwrap_or_else(|_| json!({}))` — return blocked, never silent `{}`. |
| opencode-M2 | Major | **Agree — fold** | Discovered = **`top_level_items`** (same as home Processed). Do not mix `items_total` into reconciliation lines. |
| opencode-M3 (orphan Running) | Major | **Agree — partial** | Orphan = `list_jobs` Running and snapshot idle/other job. Affordance: **Resume same id** (runner accepts Running) **or** Cancel. Not cancel-first-only. Phase-1 crash-recovery test. |
| opencode-M3 (with_ocr/neardup) | Major | **Decline** | Live `standard_body` always `stage(false, neardup_params())`. `with_ocr` does **not** enable neardup. |
| opencode-m1 | Minor | **Agree — fold** | Assert produce/qc handlers actually registered, not just the const kind list. |
| opencode-m2 + agy-m1 | Minor | **Agree — fold** | `Matter::list_item_errors_recent(&self, limit)`; `&Matter` via `open_for_read`; cap **N=100** by `updated_at`; chrome never `connection()`. |
| opencode-m3 + agy-O1 | Minor | **Agree — fold** | Extract-all **continues** on failure; UI `extract_queue`; copy “N of M extracted…”. |
| opencode-m4 | Minor | **Agree — fold** | Actor `"chrome"` = chrome-side audits only. Runner job audits stay `"system"`. |
| opencode-m5 | Minor | **Agree — fold** | `process_start` **rejects** `production_export`; chrome verb is `produce`. |
| opencode-O1 | Opportunity | **Already covered** | Finished-job counts from `list_jobs`, not snapshot. |
| opencode-O2 | Opportunity | **Decline** | Pin-count 4126 vs 4127 cosmetic. |
| agy-M1 | Major | **Agree — fold** | Named command `produce_qc_findings(root, job_id: Option<String>)` after QC terminal. |
| agy-M2 | Major | **Agree — partial** | Privilege-log.csv: **host** idempotent post-step when produce **succeeded** (from `process_progress` / `produce_page` if volume lacks log). Not WASM, not UI-only callback, not inside the runner handler. |
| agy-M3 | Major | **Agree — partial** | `process_progress(root)` isolates by **`snapshot.matter_id`** vs matter opened from `root` (`JobProgressSnapshot` has no `matter_root`). Mismatch → idle. |
| agy-m2 | Minor | **Agree — fold** | Produce wizard Busy banner → Process tab / active job. |
| agy 0121 rename | — | **Decline** | Keep folder `0121-ImageOptQcResiduals`. |

---

## 3. In scope

### 3.1 Host: one `ProcessRunner` for the EXE

- Construct `ProcessRunner` + `register_default_handlers` at `run()`. Store in Tauri managed state (`Mutex`/`Arc` as needed; `start` is `&self`).
- On app exit: `shutdown()` (join worker). Do not leak the matter worker.
- Commands that **start work** go through the runner. Short reads (`matter_overview`, `process_page`, `produce_page`) may keep `join_worker`.
- Encrypted root: `is_encrypted_matter` first — never `open_*` / `start`. Same `encrypted` error kind as 0110.
- Actor `"chrome"` on **chrome-side** audit events only (override recording, command traces). Runner-written job audit rows stay `"system"` (`profile_run` / `workflow_run` already do this). DoD honesty does **not** require `actor=chrome` on runner rows.
- Single-flight is **process-wide**: ingest Busy while produce runs (and the reverse). Surface `Busy { job_id }` in the UI; do not queue a second pipeline. Produce wizard: informative banner on `RunnerError::Busy` pointing to the Process tab / active job.
- **Forbidden deps:** `dedupe-desk`, `pst-reader`, `pst-writer`, `matter-service`. **Allowed:** `process-runner` (this track), existing chrome crates.
- Rebuild `capabilities/default.json` allow-list for new commands. No `fs:default`.

### 3.2 Process page (replaces stub)

Route `/matters/:id/process` is a live page (rename `ProcessStub` → `ProcessPage`). Four 0110 tabs still work. Admin stays stub.

**Left — Sources + profile**

- List `list_sources` (path, kind, status). Add via **existing** `tauri-plugin-dialog` (folder / file). Kinds: Purview **folder**, **ZIP**, **PST** — same as Desk. Copy must not promise OST/MBOX.
- Ingest the chosen path (`kind=ingest`). After success, inventory PSTs via `list_items_by_file_category("pst")`.
- Extract selected / extract all (`kind=extract_pst`, inventory `pst_item_id`). Sequential extract-all: UI holds `extract_queue`; dispatch the next PST when the active job is terminal. **Continue on failure** (do not halt the rest): exceptions quarantine the failed PST; siblings still extract. Copy: “N of M extracted; {name} raised K exceptions.” Do not parallelize.
- Profile: default **`builtin:standard`**, selectable among the **four builtins only**. Checkmarks **must match live stages** (OCR/neardup/FTS off on standard). **Run profile** → `profile_run`. No Save-as, no user-profile editor, no workflow_run picker (residual **D-0116-workflow**).

**Center — Jobs + exceptions**

- Jobs table from `list_jobs` (indent children when `parent_job_id` set). Columns that exist live: kind, state, counts from progress snapshot when that job is active. Do **not** invent Dupes/NIST per source if the snapshot does not have them — use `count_by_dedup_role` / overview when present, else em-dash.
- Progress: poll `process_progress` (~250–500 ms). Show `completed_count` / `total_hint` / `stage` / `message`.
- Cancel (label Pause is OK) → `process_cancel`. Resume → `process_resume` for `paused`/`failed`/`pending` **and** orphan **`running`** (same id). **Orphan Running** = `list_jobs` shows `running` and `watch_progress` is idle or a **different** `job_id` (EXE crash left a durable row). Runner `resume` of that **same** id is allowed; `start` of a different kind is `Busy` until the orphan is resumed or cancelled. Affordance on the orphan row: **Resume** (preferred) **or** Cancel — not cancel-first-only. Phase-1 tests the crash-recovery path.
- Exceptions: `item_errors` grouped by `code` (quarantine). Copy: exceptions hold **their** items; they do **not** stall ingest/extract of siblings. No password vault. Empty state is honest.

**Right — Running report + reconciliation**

- Running report: **0038 overview fields only** (items / errors / in_review / families if present). Do not hardcode mock 44,446.
- Reconciliation (normative, honest):

  | Line | Source |
  |---|---|
  | Discovered | **`top_level_items`** (same field as the home Processed chip). **Never** mix `items_total` into a visible reconciliation line. |
  | − Exceptions / quarantined | `errors.total` |
  | Review-ready | `in_review` (0 until promote) |
  | Still processing | 0 if runner idle / terminal; else remaining hint |
  | Unaccounted-for | **0** when every inventory PST has a successful `extract_pst` (or the source has no PST leaves) **and** idle. Non-zero = PST rows with no successful extract **or** a failed job without an `item_errors` row (fail closed: show the gap, do not force 0). |

  DeNIST / duplicate-instance lines: **em-dash until** those jobs have run and Desk-equivalent counters exist (`count_by_dedup_role`, cull flags). **Never** fake NSRL counts.

- Status bar: exact Hermes sentence (§2.3). Profile name = selected builtin. SHA-256 identity note is allowed; MD5 interoperability sentence is optional and must not imply MD5 is the matter identity.
- **Open review-ready** → `/matters/:id/review` (0111). Disabled when `in_review==0`.
- Download reconciliation report: **out** unless a one-liner can call existing 0039 export without new schema. Residual **D-0116-report** if skipped.

### 3.3 Host commands (Process)

Register in `generate_handler!` + capabilities.

| Command | Open | Role |
|---|---|---|
| `process_page` | **read** | sources + pst inventory + jobs + error groups + overview chips + selected-profile echo. `join_worker` OK. |
| `process_start` | **write** | `{ root, kind, params_json }` → `{ job_id }` **immediately**. `Busy` if active. Kinds allowlisted: `ingest`, `extract_pst`, `profile_run`, plus `qc` / `produce` for §3.4. **Reject** `production_export` (CLI alias; chrome verb is `produce`). Reject unknown / AI kinds from this UI (`ai_suggest_codes` is Review, not Process). |
| `process_progress` | read | Clone of `watch_progress` snapshot. Isolate by **`snapshot.matter_id`** vs the matter opened from `root` (`JobProgressSnapshot` has `matter_id`, **not** `matter_root`; `JobSnapshot` from `active_job` has both). Mismatch or none → idle. When the snapshot shows produce **succeeded** and the volume lacks `privilege-log.csv`, run the §3.4 host post-step. |
| `process_cancel` | write | `cancel(job_id)`. |
| `process_resume` | write | `resume(root, job_id)`. Same-id Running orphan is allowed. |

Register `produce_qc_findings` with produce commands (see §3.4). Rebuild capabilities allow-list.

Dialog pickers run in the **WebView plugin**, not the UI thread of a blocking rfd call on the host command.

### 3.4 Absorb D-0113-long-job (produce / QC)

Chrome Finalize / Run QC **must not** block IPC for the duration of `run_produce` / `run_production_qc`.

- `produce_qc_run` and `produce_start` become **start** commands: validate the 0113 gates **first** (blockers, overrides, membership vs last QC — **never silent re-QC**), then `process_start("qc"|"produce", params_json)` and return `{ job_id }`.
- **Do not** `create_job` in chrome produce.rs (runner Option C).
- UI polls `process_progress` until terminal; then refresh `produce_page`.
- QC **findings**: named host command **`produce_qc_findings(root, job_id: Option<String>)`** after QC reaches a terminal state. Returns the structured findings/error counts/extras for Step 5 cards. Leptos must not parse a volume path.
- Privilege-log.csv: **host** idempotent post-step when produce **succeeded**. Trigger from `process_progress` and/or `produce_page` if the volume lacks the log (app close / navigate-away must not omit it). Do **not** put the export on WASM, do **not** rely on a UI-only callback, do **not** fold it into the process-runner produce handler. Do not drop D-0040-04 chrome absorb.
- Produce/QC params: serialize chrome `intended_produce_params` (and QC equivalent) with `serde_json::to_value` **without** `unwrap_or_else(|_| json!({}))`. Serialization failure → **blocked** response, never silent `{}`. Phase-3 unit test: chrome JSON → `ProduceParams::from_json` / `QcParams::from_json` → deep-equal.
- Burn / raster commands stay `join_worker` (page-sized). Not this residual.

0113 product locks stay: DAT default, `fail_if_withheld=true`, `require_qc_pass=true`, privilege-in-set blocker, no OPT fake.

### 3.5 Desk / egui

- **`dedupe-desk` still builds** and still has Workspace Process. This track does **not** delete egui Process (Connect, unique-pst wizard **0072**, gap, encrypt-create still live there).
- Golden path DoD is **chrome-only** (no `dedupe-desk.exe` required).
- Optional one-line Desk status that Process also lives in chrome: not DoD.
- `pst-dedup-gui` legacy scan wizard: untouched.

### 3.6 Tests (normative)

Host (`cargo test -p dedupe-chrome`):

1. `process_start` ingest on a tempfile matter + tiny zip/pst fixture → job succeeds; `list_sources` ≥ 1; encrypted root refused.
2. Second `process_start` while first running → `Busy`.
3. Cancel cooperative: `process_cancel` → snapshot `paused` or `cancelled` (match 0019 vocabulary).
4. `profile_run` `builtin:standard` on a matter with at least one extracted item (or extract_only fixture) → child jobs exist; no schema bump.
5. Produce/QC start returns `job_id` without waiting for engine completion (mock/fixture small still OK); `create_job` is not called from chrome produce.rs. Chrome produce/QC JSON round-trips `ProduceParams::from_json` / `QcParams::from_json` deep-equal. No `json!({})` silent fallback.
6. Unaccounted-for: after ingest+extract of one fixture PST, idle → 0; with an inventory PST and no extract → non-zero. Discovered uses `top_level_items` only.
7. `register_default_handlers` actually registered `produce` and `qc` (not merely the const kind list).
8. Crash-recovery: durable `Running` row with idle snapshot → `process_resume` same id **or** `process_cancel` unblocks a later `process_start`.
9. `process_start(..., "production_export")` rejected. `process_progress(root)` for a different matter than the snapshot returns idle.
10. `list_item_errors_recent` on `&Matter` (read-only open), cap 100.

UI (`chrome-ui` wasm): Process page is not the stub string; status-bar sentence present. No `process-runner` in the ui crate.

Existing 0111–0115 chrome tests still pass.

---

## 4. Out of scope (do NOT do here)

- Unique-pst / unique-eml / keep-set (**0071/0072/Series K–S**). CLI/Desk wizard stay.
- `workflow_run` chrome UI (**D-0116-workflow**).
- User processing-profile editor / NSRL RDS import (**D-0024-01**).
- OST/MBOX/7z ingest (**D-0016-05**).
- First-pass AI in Process (**0051** is Review, off by default).
- Admin tab, Connect/SSO, encrypt-create, matter-service.
- Queue/window/produce-wizard/raster UI Bugbot (**0117–0120**).
- Image OPT QC eligibility (**0121**).
- WASM rewrite of jobs. Second ingest pipeline. Daemon.
- BCC-default. Schema v42. Gutting `dedupe-desk`.
- Drag-drop residual (**D-0116-drop**) unless dialog+path is insufficient for DoD.

---

## 5. Preconditions & dependencies

- **P1 (blocking):** 0110–0115 Completed; `process-runner` `register_default_handlers`; schema v41.
- *Verified to date:* ProcessStub live; chrome has no process-runner; produce/QC use `join_worker` + `create_job`; Desk hosts the runner; `builtin:standard` stages as §2.2; dialog plugin already inited; `list_items_by_file_category("pst")` is pub.

---

## 6. Risks

| Risk | Mitigation |
|---|---|
| Chrome EXE size / compile time from default process-runner features | Accept (Desk already pays this). Do not split a second handler set. |
| Double `create_job` (chrome + runner) | Forbid chrome `create_job` for kinds the runner owns. Tests grep/assert. |
| Produce wizard loses findings / privilege log | Named `produce_qc_findings`; host idempotent privilege-log post-step on produce success. |
| Busy collide Process vs Produce | One runner; Produce wizard banner → Process tab. |
| Cross-matter progress leak | `process_progress` idle when `snapshot.matter_id` ≠ opened matter. |
| Orphan Running wedges start | Resume same id or Cancel; Phase-1 crash-recovery test. |
| Silent empty produce JSON | Delete `json!({})` fallback; blocked on serialize fail; round-trip test. |
| Mock OST/NSRL copied into UI | Spec forbids; DoD copy review. |
| Unaccounted-for forced to 0 | Fail closed: show gap when extract missing. |
| `expect` on runner mutexes | New chrome code: no `unwrap`/`expect`. Existing process-runner internals out of scope unless touched. |
| wasm job crate leak | `ui/` exclude; no process-runner dep on ui package. |

---

## 7. Definition of Done

Complete only when ALL hold:

- [ ] **DoD-1 — Process page is live:** `/matters/:id/process` is not the stub. Sources + builtins + jobs + exceptions + reconciliation + status-bar sentence. Four tabs still work. Queue / 0112 / produce / raster still work. `dedupe-desk` still builds.
- [ ] **DoD-2 — Golden path without Desk:** tempfile matter → Add PST or ZIP → ingest → extract → `profile_run` `builtin:standard` (or `extract_only` then promote if fixture has no office) → home **Processed > 0**. Unaccounted-for **0** when idle after successful extract of all inventory PSTs. Encrypted root refused.
- [ ] **DoD-3 — One runner:** `process-runner` on the host; `start` returns before the handler finishes; cancel works; second start is `Busy`; `register_default_handlers` **actually** registered produce/qc; shutdown on exit. Chrome does not `create_job` for runner kinds. Orphan Running: resume same id or cancel. `process_start` rejects `production_export`. `process_progress` isolates by `matter_id`.
- [ ] **DoD-4 — Long jobs:** `produce_qc_run` / `produce_start` no longer block for the engine duration (`D-0113-long-job` closed). 0113 gates unchanged. Privilege-log.csv still written on produce success via **host** idempotent post-step. `produce_qc_findings` loads wizard findings after QC terminal. Produce/QC params round-trip; no silent `{}`. Produce wizard shows Busy banner → Process.
- [ ] **DoD-5 — Honesty:** exceptions from `item_errors` (`list_item_errors_recent`, cap 100) do not stall sibling extract (extract-all **continues**); Discovered = `top_level_items`; no OST/MBOX/NSRL/7z fakes; no `unwrap`/`expect` in new production chrome/host; no schema bump; no `connection()` in chrome. Actor `"chrome"` is chrome-side only.
- [ ] **DoD-6 — Recorded:** `review.md`; registry **Completed**; CHANGELOG Unreleased sentence; `D-0116-process-fold` and `D-0113-long-job` closed; ledger committed (`FEATURE`). **0117–0121** stay Proposed unless separately implemented.

---

## 8. Verification commands (reference)

```powershell
Set-Location C:\dev\Dedupe
cargo test -p process-runner
cargo test -p dedupe-chrome
cargo check -p pst-dedup-cli
cargo check -p pst-dedup-gui
cargo check -p dedupe-desk
# chrome-ui job equivalent (re-verify workflow file at execute)
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
ledgerful verify
```

Do **not** `git add` operator PSTs or `output/`.

---

## 9. Deferred absorb / decline

| ID | Disposition |
|---|---|
| **D-0116-process-fold** | **Absorb — this track.** |
| **D-0113-long-job** | **Absorb — §3.4.** |
| **D-0116-workflow** | **Mint** if workflow picker skipped (expected). |
| **D-0116-drop** | **Mint** if drag-drop skipped (dialog is enough). |
| **D-0116-report** | **Mint** if 0039 download skipped. |
| **D-0020-01** | Decline (operator GUI smoke). |
| **D-0016-05** | Decline (7z). Show `unsupported_7z` if ingest returns it. |
| **D-0024-01** | Decline (NSRL RDS). No fake NSRL copy. |
| **D-0019-01** / **D-0044-02** | Decline (true parallel stages). |
| **D-0110-deny-unic** | Remain (upstream unic). |
| **D-0117 … D-0120** | Remain on those placeholders. |
| **D-0121** | **Minted** from PR #121 Bugbot. Not this track. |
| **D-0062-codesign** | Remain. |
| **D-0108-keepset-crc-retaint** | Decline (unique-pst). |
| BCC-default | Never. |
| opencode-M3 with_ocr/neardup | **Decline** — live neardup stays false in `standard_body`. |
| opencode-O2 pin-count | **Decline** — cosmetic. |
| agy 0121 rename | **Decline** — keep `0121-ImageOptQcResiduals`. |

---

## 10. Unblocks

Counsel can process a matter in the chrome EXE. Closes the Series O spine. Residual chrome Bugbot tracks (**0117–0120**) and **0121** stay independent.
