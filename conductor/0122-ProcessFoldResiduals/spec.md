# 0122 — Process-fold residuals (PR #123 Bugbot)

> Structure follows `templates/0000-Description/spec.md`. DoD is §7.
> Phase 0 is **closed** in this file. Do not re-open unique-export, matter-home
> (**0110**), first-pass queue (**0111** / **0117**), review-window async
> (**0118**), DAT produce wizard honesty (**0113** / **0119**), zpdf burn
> compose (**0114**), Image-tab overlay/Burn counts (**0120**), image OPT QC
> (**0121**), or Series T canvas (**0123–0126**). Do not vendor
> `C:\dev\dedupe-frontend`. Do not mint a BCC-default track.

- **Track ID:** 0122-ProcessFoldResiduals
- **Execution repo:** `C:\dev\Dedupe`
- **Governance:** this directory in `C:\dev\Dedupe\conductor\` (track registry: `../conductor.md`)
- **Plan-of-record reference:** Hermes Process workspace. `C:\dev\Dedupe-plan.md` is **absent** (re-verified 2026-09-01); do **not** chase it at execute.
- **Cross-repo contract:** mock at `C:\dev\dedupe-frontend` is research only (jobs **table** is **0126**; shell tokens are **0123**).
- **Status:** In progress
- **Depends on:** **0116 Completed** (PR **#123** / `727c857`) · **0119 Completed** (do not retouch cancelled-produce) · schema **v41** (no bump)
- **Spec authored:** 2026-09-01 (placeholder → Ready)
- **Series:** O (Review chrome) — PR #123 Process extract-all / orphan-row residual
>
> **Closes / absorbs:** `D-0122-process-fold-residuals` (this track). Does **not** close D-0123–D-0126, D-0116-workflow / D-0116-drop / D-0116-report, D-0062-codesign.
> **HITL:** owner launches the **release** chrome EXE on a synthetic matter with **≥2** inventory PSTs: Extract all, click Extract all again while the first extract is running — remaining PSTs must still dispatch after the in-flight job. Pause the in-flight extract mid-batch — remaining queue must **not** auto-restart. A live `running` job row must show **Pause** (not orphan Resume) once `process_progress` has the matching `job_id`. INC* unique-pst is **not** a gate. Codesign is **D-0062-codesign**.
>
> **Last-PR fold-in (2026-09-01):** PRs **#136, #135, #134, #133**. Disposition in §2.8. No new mint. Next free ID **0127**.
>
> **Placeholder opencode-review (2026-08-31):** absorbed into this Ready pass (M1 Busy string contract; M2 drain wipe; m1 frozen `counts`; m2 guard before any signal writes; m3 pure helpers + ui `#[test]`; m4 HITL inspects the **row** Pause). Not a new product mint.
>
> **Fold-in (2026-09-01):** `opencode-review.md` + `agy-review.md` of this Ready spec. OpenCode **M1** Busy-retry is **event-driven** (`busy_retry_pending`), not “snapshot not busy / idle”. Agy M1–M3 already in scope. See §9 + `foldin-note.md`.
>
> **Stack lock (inherit 0110–0121):** Tauri **2** + Leptos **0.8 CSR** on **stable** Rust. Plex / paper / cool chrome. Red = privilege / withhold / blocker / draft redact overlay only. No daemon. No schema bump. `ui/` stays workspace-excluded. One pipeline (`process-runner`). 0119 `volume_succeeded` / `process_job_succeeded` unchanged. 0121 OPT/sniff unchanged. Default DAT-only and `qc_default_v1` unchanged.

---

## 1. Objective

Keep **0116** Process **honest** under single-flight extract-all and live job rows: a Busy second Extract all must not drop the remaining PST queue, and a `running` job must not stay painted as an orphan because `orphan` / `active` were captured when the `For` child mounted.

This is **correctness**. Silently dropping remaining extracts is the same honesty class as a silent unique-export drop. Unique-export itself is unchanged.

---

## 2. Context (read before starting)

### 2.1 Why this track, now

**0116 Completed** (PR **#123** / `727c857`) folded Process into chrome + `process-runner`. Two **valid** Cursor Bugbot findings on `process.rs` were parked here so **0117** could proceed. The third #123 finding (cancelled produce treated as success) is **0119 Completed** — do not reopen. **0126** owns jobs-table layout — keep Pause/Resume/Cancel honesty here.

### 2.2 Live APIs (plan-time 2026-09-01, HEAD `8628131`; re-verify at execute)

| Surface | Fact |
|---|---|
| `crates/matter-core/src/schema.rs` | `SCHEMA_VERSION == 41`. **No schema bump this track.** |
| `extract_all` (`ui/.../process.rs` ~382–428) | Rebuilds `extract_queue` from full inventory, zeroes `extract_done` / `extract_note`, starts first `extract_pst`. **Any** `process_start` Err (including Busy) does `extract_queue.set(Vec::new())` + `extract_total.set(0)`. No guard for an already-draining queue. |
| Drain (~264–293) | On extract finish, `q.remove(0)` then `process_start` next. **Any** next-start Err also `extract_queue.set(Vec::new())`. Same Busy wipe. |
| `is_orphan_running` (~133–136) | `job.state == "running"` and snapshot idle / other `job_id`. Definition is **fine**. |
| Job `For` (~546–582) | `let snap = progress.get(); let orphan = …; let active = snap.job_id == j.id;` then `<Show when=move \|\| orphan>` / `active && snapshot_busy(...)`. `orphan`/`active`/`counts` are **one-shot** at child create. `For` does not re-run `children` on `progress` poll. Pause Show re-reads `progress` but ANDs a frozen `active`. |
| Poll interval | `process_progress` every **400** ms (~300–305). A `running` row can mount before the first poll. |
| `tauri_invoke` Err | `String`. Busy host error Display is `busy: matter is busy: a job is already running ({job_id})` (`error.rs` `CommandError::busy` + `Display` `"{kind}: {message}"`). `produce.rs` already classifies with `is_busy_err`: `starts_with("busy:")` **or** `contains("matter is busy")`. Copy that predicate into `process.rs` (do not change produce). |
| `reject_if_busy` / `process_start_blocking` | Single-flight. **Do not** change runner Busy semantics. Host Busy tests stay. |
| `process.rs` ui crate | **No** `#[cfg(test)]` module today. Add helpers + unit tests (same pattern as `produce.rs`). |
| MS-PST | **N/A this track.** |

### 2.3 Mock + Hermes (research only)

Process workspace: extract + jobs. Do not steal 0126 table/minus-stack or 0123 shell.

### 2.4 Plan-time crate pins (re-verify at execute)

| Crate | Plan-time | Rule |
|---|---|---|
| `tauri` | **2** | Reject **3.x / pre-release**. |
| `leptos` | **0.8.x** CSR | Do not bump major. |
| Schema | **41** | No bump. |
| Rust | **stable** (CI) | No nightly. |
| trunk | **0.21.14** (ci.yml) | Keep. |

### 2.5 Tools / recall

Ran from `C:\dev\Dedupe`:

- `ai-brains preflight --summary` (inited; **4176** pinned at fold-in 2026-09-01; plan-time was 4175).
- Recall: 0117 Ready minted these two Process items here (`50db2cfe`); 0116 Busy single-flight (`f4b60830`). Do not change runner Busy.
- `ledgerful doctor --json` `readyForPublish: true`; warns phantom-promote, completion-unreachable, sig-pin, sig-version (impact-stale was transient).
- Ledger compact: **0 pending / 0 unaudited drift**. Execute: git-commit these conductor/docs first (owner), then start `0122-process-fold-residuals` **BUGFIX** for product code only.
- `ledgerful scan --impact` **LOW** on stray untracked root files (`.claude`, repo-root `agy-review.md`, `fixtures/keep_set_summary.json`). Do not `git add` those. Federated scan hit the 5000-file budget under `output/` — ignore.

### 2.6 What we could not verify

Owner HITL on the release EXE (second Extract all + Pause vs orphan Resume). Execute re-reads `For` children vs `progress` on live Leptos 0.8.

### 2.7 Related deferred (roll)

See §9. Absorb **D-0122**. Remain D-0116-workflow / drop / report (0126). Decline D-0032-08 / D-0020-01 as operator smoke.

### 2.8 Last-PR Cursor comments (2026-09-01)

PRs **#136, #135, #134, #133**. Inline review comments empty. Issue comments are Bugbot **usage-limit** only. **Decline** as product input.

Origin PR **#123** still has three Bugbot items: cancelled-produce → **0119** (done); extract-all Busy + live-orphan → **this track** (live-verified at §2.2). Additional location for Busy wipe: drain `~263–292`.

| Origin | Verdict |
|---|---|
| #123 High — cancelled produce as success | **Remain 0119** (Completed). Do not steal. |
| #123 Medium — Extract-all Busy wipes queue | **Absorb** |
| #123 Medium — Live jobs shown as orphans | **Absorb** |
| #136–#133 usage-limit | **Decline** |

No new mint. Next free ID **0127**.

### 2.9 Product locks (do not invent at execute)

**Extract-all Busy (Medium 1).**

1. Guard at the **top** of `extract_all`, **before** any writes to `extract_queue` / `extract_total` / `extract_done` / `extract_note` / `extract_current_name`. If the queue is non-empty **or** `snapshot_busy(&progress)`: **silent no-op** (or a short note). Do not rebuild/zero. Do not call `process_start`. Kind need not be `extract_pst` — any busy snapshot blocks a second Extract all.
2. On `process_start` Err from extract-all **or** the drain next-start (`~288`): if Busy (`is_busy_invoke_err`, same predicate as `produce.rs` `is_busy_err`), **do not** clear `extract_queue` / `extract_total`. Set `busy_retry_pending`. Other errors may still clear the queue, clear the flag, and surface `error`.
3. **Busy retry is event-driven, not snapshot-state.** `snapshot_busy` is false for `paused` / `cancelled` / `succeeded` / `failed` / `idle` (live `process.rs`). `process_progress_blocking` synthesizes `idle` only when `job_id` is empty or the matter differs — after a job in this session the poller is **not** idle. Therefore:
    - **Do not** retry on “queue non-empty && snapshot not busy.” That auto-starts `q.first()` after operator Pause (drain skips `remove(0)` on `finished_paused`, so the paused PST is still head).
    - **Do not** retry on “snapshot idle.” That never fires after the first job.
    - Set `busy_retry_pending` **only** when a start / drain-next-start fails `is_busy_invoke_err`.
    - On a later poll, fire **one** `process_start` of `q.first()` **without** another `remove(0)` only while the flag is set **and** `!snapshot_busy`.
    - **Clear** the flag on successful start, on `finished_paused` (pause or cancel — never auto-restart), and when a non-Busy error clears the queue.
    - Do not change 400 ms polling or runner Busy.
4. Optional honesty: disable Extract all while `snapshot_busy` **or** `busy_retry_pending`. Do **not** disable solely because a leftover queue remains after Pause (no new clear-queue control this track). Guard (item 1) still no-ops a click while the queue is non-empty.

**Orphan / active (Medium 2).** Keep `is_orphan_running`. Do **not** capture `orphan`, `active`, or `counts` from a one-shot `progress.get()` in the `For` `children` closure. Read `progress` **inside** reactive `Show` / view closures (clone `ProcessJobRow` / `job_id` into them). After poll, a row whose `id` equals `progress.job_id` and `snapshot_busy` shows **Pause**, not Resume/Cancel. True orphans (running in `page.jobs`, snapshot idle or other id) still get Resume + Cancel (0116 lock).

Do not change `process-runner` Busy, `reject_if_busy`, 0119 produce waits, 0121 QC, or 0126 layout.

---

## 3. In scope

`crates/dedupe-chrome/ui/src/pages/process.rs` extract-all queue + job-row reactivity. Unit tests in that file. Host only if a tiny Busy-string helper needs a shared test — prefer ui crate.

### 3.1 Extract-all does not wipe on Busy

Pure helpers (unit-tested): `extract_all_should_start(queue_len, snapshot_busy)`, `is_busy_invoke_err` (same as `produce.rs` `is_busy_err`), `should_clear_queue_on_start_err`, `should_set_busy_retry`, `should_fire_busy_retry(pending, snapshot_busy)`, `should_clear_busy_retry` (success / `finished_paused` / non-Busy clear). Wire into `extract_all` and the drain match. Guard `extract_all` before any signal writes.

### 3.2 Job row orphan/active from current progress

Reactive Pause / Resume / counts as §2.9. Keep `is_orphan_running` table tests (idle snap vs matching running snap).

---

## 4. Out of scope (do NOT do here)

- Cancelled/idle produce as success (**0119** Completed).
- Queue virtualization (**0117**).
- Process jobs **table**, drop copy, minus-stack, report download (**0126** / D-0116-*).
- Matter shell (**0123**). Image OPT QC (**0121**). Raster overlay (**0120**).
- Changing `process-runner` single-flight / Busy mapping.
- Clear-queue control after Pause, or changing `finished_ok` so pause/cancel does not increment `extract_done` (0116 pre-existing; OpenCode O1/O2).
- Schema bump, BCC-default, WASM jobs, deleting Desk Process.

---

## 5. Preconditions & dependencies

- **P1 (blocking):** **0116 Completed**. Schema **41**.
- *Verified to date:* both Bugbot sites still present on HEAD `8628131` (§2.2).

---

## 6. Risks

| Risk | Mitigation |
|---|---|
| Busy next-start leaves remaining PSTs never started | `busy_retry_pending` set only on Busy start failure; poller fires `q.first()` while flag set and `!snapshot_busy`. |
| Pause/cancel auto-restarts remaining queue | “Not busy” includes `paused`. Clear the flag on `finished_paused`; never retry from snapshot state alone. |
| Disabling Extract all forever after Pause | Disable only while busy or retry-pending, not solely because leftover queue is non-empty. |
| True crash orphans lose Resume | Keep 0116 `is_orphan_running`; only the **read site** changes. |

---

## 7. Definition of Done

Complete only when ALL hold:

- [ ] **DoD-1 — Extract-all Busy:** Second Extract all while a queue is draining or the runner is busy does **not** empty `extract_queue` and does **not** reset totals/notes. Remaining PSTs still dispatch after the in-flight job. Drain next-start Busy does not wipe the rest. Pause/cancel mid-batch does **not** auto-start `q.first()`. ui unit tests for Busy keep-queue **and** the `busy_retry_pending` state machine (`is_busy_invoke_err` matches `busy:` / `matter is busy`).
- [ ] **DoD-2 — Live row Pause:** A `running` job whose snapshot `job_id` matches the row shows **row** Pause (not orphan Resume, not only the status-bar Pause) once progress has polled. `orphan`/`active`/`counts` are not mount-time bools. `is_orphan_running` unit tests still cover true orphans.
- [ ] **DoD-3 — Hygiene:** No `unwrap`/`expect` in new production code. No schema bump. `process-runner` Busy unchanged. Host Busy `kind == "busy"` tests still pass; if `CommandError::busy` Display is untested, add one `starts_with("busy:")` assert (do not change runner). `cargo test --manifest-path crates/dedupe-chrome/ui/Cargo.toml` + `cargo test -p dedupe-chrome`. 0119 latch tests still pass.
- [ ] **DoD-4 — Recorded:** `review.md`; registry **Completed**; CHANGELOG Unreleased sentence; `D-0122-process-fold-residuals` closed; ledger committed (`BUGFIX`). **0123–0126** stay Proposed unless separately implemented.

---

## 8. Verification commands (reference)

```powershell
Set-Location C:\dev\Dedupe
cargo test --manifest-path crates/dedupe-chrome/ui/Cargo.toml
cargo test -p dedupe-chrome
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
ledgerful verify
```

Do **not** `git add` operator PSTs, `output/`, stray `agy-review.md`, or `fixtures/keep_set_summary.json`.

---

## 9. Deferred absorb / decline

| ID | Disposition |
|---|---|
| **D-0122-process-fold-residuals** | **Absorb — this track.** |
| **D-0116-workflow** | Remain (0126 / later). |
| **D-0116-drop** | Remain (**0126** copy). |
| **D-0116-report** | Remain (**0126**). |
| **D-0121-image-opt-qc** | Closed in **0121**. |
| **D-0123-matter-shell** | Remain (**0123**). |
| **D-0126-process-chrome-visual** | Remain (**0126**). Do not restyle the jobs table here. |
| **D-0119-produce-checklist-residuals** | Closed in **0119**. Do not reopen cancelled-produce. |
| **D-0032-08** | Decline (operator GUI smoke). |
| **D-0020-01** | Decline (operator GUI smoke). |
| **D-0062-codesign** | Remain. |
| Bugbot usage-limit on #133–#136 | **Decline**. |
| PR #123 extract-all + orphan | **Absorb** (this track). |
| PR #123 cancelled produce | **Remain 0119** (done). |
| BCC-default | Never. |
| OpenCode M1 (Busy retry predicate) | **Agree — fold** — `busy_retry_pending`; not “not busy” / not “idle”. |
| OpenCode m1 (pin 4176) | **Agree — fold** — §2.5 refreshed. |
| OpenCode m2 (uncommitted planning docs) | **Agree — partial** — plan Phase 0 / handoff: owner git-commits conductor+deferred before product BUGFIX. Foldin does not git-commit. |
| OpenCode O1 (clear-queue after Pause) | **Decline** — leftover queue until Resume is 0116; no new control. |
| OpenCode O2 (`finished_ok` counts pause) | **Decline** — 0116 pre-existing; §4. |
| Agy M1 / M2 / M3 | **Already covered** (guard, reactive For, drain keep-queue + retry). |
| Agy m1 (disable Extract all while queue non-empty) | **Agree — partial** — disable while busy or retry-pending, not solely leftover-after-Pause. |
| Agy m2 (Display `busy:`) | **Already covered** — DoD-3. |
| Agy O1 (true-orphan Resume) | **Already covered** — keep `is_orphan_running`. |

---

## 10. Unblocks

Counsel can Extract all a multi-PST inventory without a second click deleting the rest of the queue. A live running job shows Pause once progress has the id. **0126** can restyle the jobs table on top of this honesty.
