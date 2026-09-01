# 0119 — Produce-checklist residuals (PR #117 / #123 Bugbot)

> Structure follows `templates/0000-Description/spec.md`. DoD is §7.
> Phase 0 is **closed** in this file. Do not re-open unique-export, matter-home
> (**0110**), first-pass queue (**0111** / **0117**), review-window async
> (**0118**), zpdf burn compose (**0114**), Image-tab mouseup/draw-state/Burn
> counts (**0120**), TIFF/OPT QC (**0115** / **0121**), Process extract-all
> (**0116** / **0122**), or produce **canvas** layout (**0125**). Do not vendor
> `C:\dev\dedupe-frontend`. Do not mint a BCC-default track.

- **Track ID:** 0119-ProduceChecklistResiduals
- **Execution repo:** `C:\dev\Dedupe`
- **Governance:** this directory in `C:\dev\Dedupe\conductor\` (track registry: `../conductor.md`)
- **Plan-of-record reference:** Hermes produce checklist. `C:\dev\Dedupe-plan.md` is **absent** (re-verified 2026-08-31); do **not** chase it at execute.
- **Cross-repo contract:** mock at `C:\dev\dedupe-frontend` is research only (layout is **0125**).
- **Status:** In Progress
- **Depends on:** **0113 Completed** (PR **#117** / `f192b2d`) · **0116 Completed** (process-runner produce/QC) · schema **v41** (no bump)
- **Spec authored:** 2026-08-31 (placeholder → Ready)
- **Series:** O (Review chrome) — PR #117 / #123 produce honesty residual
>
> **Closes / absorbs:** `D-0119-produce-checklist-residuals` (this track). Does **not** close D-0120–D-0126, D-0031-03, D-0040-04, D-0040-10, D-0060-03 (partial hint only), D-0020-01, D-0062-codesign.
> **HITL:** owner launches the **release** chrome EXE, synthetic matter with a produceable set: Finalize once → second Finalize must not write the same Bates; switch matter → QC cards gone; cancel from Process while producing → wizard must **not** show a completed volume. INC* unique-pst is **not** a gate. Codesign is **D-0062-codesign**.
>
> **Last-PR fold-in (2026-08-31):** PRs **#128, #127, #126, #125**. Disposition in §2.8. No new mint. Next free ID **0127**.
>
> **Harness fold-in (2026-08-31):** `opencode-review.md` + `agy-review.md`. Centerpiece: dedicated `volume_succeeded` latch (QC must not re-arm Finalize); empty-union blank count **0** is accepted; §3.2 depends on §3.1. Status stays **Ready — not started**.
>
> **Stack lock (inherit 0110–0118):** Tauri **2** + Leptos **0.8 CSR** on **stable** Rust. Plex / paper / cool chrome. Red = privilege / withhold / blocker only. No daemon. No schema bump. `ui/` stays workspace-excluded. One pipeline. 0113 privilege-in-set / `fail_if_withheld=true` / `require_qc_pass=true` unchanged.

---

## 1. Objective

Keep the **0113** produce wizard **honest after Finalize, across matter switches, and under cancel**: a second click must not stamp the same Bates onto a second volume; `filter_ids: Some([])` must not dump the privilege-log corpus; QC cards must not leak between matters; a cancelled or idle produce/QC job must not be painted as a successful volume.

This is **correctness**, not chrome polish. Colliding control numbers and a corpus-wide privilege log are the same honesty class as a silent unique-export drop. Unique-export itself is unchanged.

---

## 2. Context (read before starting)

### 2.1 Why this track, now

**0113 Completed** (PR **#117**) shipped the DAT wizard. Three **valid** Cursor Bugbot findings were parked here so **0114** could proceed. **0116** moved produce/QC onto `process-runner` and added PR **#123** cancelled-produce-as-success. **0118 Completed** (PR **#127**). **0125** owns un-wizard layout — not this ID.

### 2.2 Live APIs (plan-time 2026-08-31, HEAD `1b32854`; re-verify at execute)

| Surface | Fact |
|---|---|
| `crates/matter-core/src/schema.rs` | `SCHEMA_VERSION == 41`. **No schema bump this track.** |
| `ui/src/pages/produce.rs` Finalize `disabled` (~717–744) | Gates `start_busy`, `bates_start >= 1`, QC blockers, empty warn overrides. **Does not** read `start_result.ok`. After a succeeded wait, Finalize is clickable again with the same typed Bates. |
| `wait_process_terminal` (~32–51) | Returns on `idle` / empty `job_id` / `succeeded` / `failed` / `cancelled` / `paused`. Produce (~260–268) and QC (~144–157) treat only `failed` **or** `paused` as error; **`Ok(_)` includes `succeeded`, `cancelled`, and `idle`.** |
| Produce cancel in runner | **`crates/process-runner/src/handlers/produce.rs:93-94`** maps `ProduceOutcome::Paused` → snapshot **`paused`** (message `"cancelled"`). Same `paused` + `"cancelled"` mapping lives in sibling handlers — do **not** change it. `paused` is already an error in the UI match. **`idle` after cancel** (progress watch reset / matter mismatch in `process_progress_blocking`) still hits the success arm. |
| `produce_start_blocking` | Returns `ok: true` + `job_id` **before** the job finishes (0116). UI must not treat start-`ok` as volume-ok. |
| Route Effect (~93–116) | Reloads `produce_page`. Sets `root_sig`, `page`, prefix/profile. **Does not** clear `qc`, `overrides`, `step`, `entire_corpus`, `start_result`, `bates_start`, `start_busy`. |
| `next_seq_hint` | `produce_page` returns it; UI displays set `next_seq` but **does not** write `bates_start` from the hint after success. |
| `matter-core` `export_privilege_log` / `count_privilege_log_blank_descriptions` (~928–935, ~987–994) | `if let Some(ids) = filter_ids { if !ids.is_empty() { IN (…) } }` — **`Some([])` is unfiltered**, same as `None`. |
| Chrome `write_privilege_log_for_volume` (~655–664) | Always `filter_ids: Some(produced ∪ withheld-in-scope)`. Empty union still `Some(vec![])`. |
| `privilege_log_blank_blocker` (~337–339) | Same union into `count_…(Some(&filter_ids))`. Empty union can count **corpus** blanks. |
| Privilege-log post-step | `process_progress` calls `ensure_privilege_log_after_produce` only when `kind==produce && state==succeeded`. Log failure must not re-arm a second produce. |
| MS-PST | **N/A this track.** |

### 2.3 Mock + Hermes (research only)

Mock three-pane canvas is **0125**. Steal nothing from Archivo/coral. 0113 locks stay: privilege-in-set blocker, DAT default, no fake categorical log (**D-0031-03**).

### 2.4 Plan-time crate pins (re-verify at execute)

| Crate | Plan-time | Rule |
|---|---|---|
| `tauri` | **2** | Reject **3.x / pre-release**. |
| `leptos` | **0.8.x** CSR | Do not bump major. |
| Schema | **41** | No bump. |
| Rust | **stable** (CI) | No nightly. |

### 2.5 Tools / recall

Ran from `C:\dev\Dedupe`:

- `ai-brains preflight --summary` (inited; **4132** pinned at fold-in; plan-time recorded 4131 — cosmetic +1).
- Recall: 0113 `filter_ids = produced ∪ withheld-in-scope` (never produced-only); warning overrides are session/payload; QC and produce share `item_ids` + pack.
- `ledgerful doctor --json` `readyForPublish: true`; scan `--impact` **LOW**. Doctor warns (plan-time + fold-in): phantom-promote, sig-pin, sig-version, completion-unreachable — none block. `"impact-stale"` is no longer emitted.
- Unrelated pending ledger txs (plan-time: `how-to-build` Docs) — **do not** commit them from this track.

### 2.6 What we could not verify

Owner HITL on a live cancel-from-Process vs idle snapshot. Execute re-reads `process_progress` after `process_cancel` on a produce job.

### 2.7 Related deferred (roll)

See §9. Absorb **D-0119**. Partial **D-0060-03** (apply `next_seq_hint` **after success** only). Decline D-0031-03 / D-0040-04 folder / D-0040-10 / canvas **0125**.

### 2.8 Last-PR Cursor comments (2026-08-31)

PRs **#128, #127, #126, #125**. Issue comments are Bugbot **usage-limit** only (no findings). Review comment arrays empty. **Decline** as product input. No new mint. Next free ID **0127**.

### 2.9 Product locks (do not invent at execute)

- `None` filter_ids = unfiltered (Desk/CLI whole-scope). **`Some([])` = empty set (0 rows).** Do not make `None` empty.
- Union remains produced ∪ withheld-in-scope.
- Privilege-in-set / `fail_if_withheld` / `require_qc_pass` unchanged.
- `produce_start` may still return `ok: true` + `job_id` when **gates** pass; volume success is **job `succeeded`**.
- Do not rewrite the five-step wizard into 0125’s canvas.

---

## 3. In scope

UI + `matter-core` filter semantics + chrome host tests. **Do not** change produce engine Bates assignment, QC pack JSON, or Process extract-all (**0122**).

### 3.1 Terminal snapshot is success only on `succeeded`

Shared helper (ui crate, unit-tested):

```text
fn process_job_succeeded(state: &str) -> bool
  // true iff state == "succeeded" (exact, case-sensitive wire form)
```

Produce wait (~260) and QC wait (~144):

- **Success:** `process_job_succeeded(&snap.state)` → refresh page / apply findings.
- **Not success:** `failed`, `paused`, `cancelled`, `idle`, empty `job_id`, anything else → error/status; **do not** set volume-ok; **do not** call `produce_qc_findings` for a non-succeeded QC job.
- After **every** `await`, ignore the result if captured `root` ≠ `root_sig.get_untracked()` (matter switched mid-flight).

Do **not** change runner `Paused` mapping. The bug is the UI treating the wait catch-all as ok.

### 3.2 Finalize disarmed after a successful volume

**§3.1 lands first.** Today the produce `Ok(_)` arm (produce.rs:~268) fills `start_result` for *every* terminal (`succeeded` / `cancelled` / `idle`). After §3.1 that fill runs only on `succeeded`. Do not key `disabled` off `start_result.ok` against the old shared fill.

Live QC success already does `start_result.set(None)` (~171 / ~178). Keying disarm off `start_result.ok` would **re-arm Finalize after a QC re-run**. Use a dedicated session latch instead.

- Dedicated latch `volume_succeeded` (name at execute; **not** QC’s `start_result`):
  - Set **true** only in the produce wait’s §3.1-succeeded refresh — the **same** block that applies `next_seq_hint`.
  - **Not** cleared by QC.
  - A not-succeeded produce terminal **leaves the latch as-is** (prior success stays disarmed; a cancel of a first attempt stays unlatched so Finalize can retry).
  - Cleared on matter switch (§3.4).
- `disabled` is true when `volume_succeeded` **or** `start_busy` (plus existing Bates / QC / override gates).
- `next_seq_hint` → `bates_start` happens **only** in that succeeded refresh, never in the not-succeeded branch and never on every page poll. First paint still requires an explicit ≥ 1 start (**D-0060-03** not fully closed; no silent `1`).
- DoD-2 **primary** exit: Finalize `disabled` after succeeded. Hint fill is a **required UI affordance** after that success, not an alternate way to skip the disable.
- Privilege-log post-step: `ensure_privilege_log_after_produce` already returns **Ok** on persistent `already open` after 20 retries (produce.rs:611–614). Surface **genuine** kinds (`encrypted`, `not_found`, other). Do **not** fail progress polls by “fixing” that silent-Ok. After `succeeded`, Finalize stays disarmed even if the log write later errors; retry is log-only (`ensure_privilege_log_if_missing`).

### 3.3 `Some([])` is empty-set in privilege log

In **both** `export_privilege_log` and `count_privilege_log_blank_descriptions`:

| `filter_ids` | Meaning |
|---|---|
| `None` | Unfiltered (existing Desk/CLI). Keep. |
| `Some([])` | **Zero** rows / blank count **0**. Add `AND 0` (or skip the query and return empty). |
| `Some(non-empty)` | `IN (…)` as today. |

Host still passes `Some(union)`. Tests: tempfile — `None` still exports eligible rows; `Some(vec![])` exports 0 and blank count 0; `Some(vec![one_id])` still filters.

**Accepted:** empty-union blank count becomes **0** (no corpus-wide alarm at QC on an empty union). Today `privilege_log_blank_blocker` accidentally counts corpus blanks when the union is empty; that alarm is **retired**. Corpus-wide blank surfacing remains available via `None` (Desk/CLI) — chrome volume/QC must **not** pass `None`.

Do **not** pass `None` from chrome volume export.

### 3.4 Matter switch clears wizard session

When route `id` / `root` changes (same Effect that loads `produce_page`):

Clear **before or with** the new page: `qc`, `overrides`, `start_result`, `volume_succeeded` latch, `step` (back to 1), `entire_corpus` (false), `bates_start` (empty — operator types ≥ 1 again), `busy_banner`, `error`. Re-apply prefix/profile from the **new** `produce_page`. Do not keep previous matter’s `ordered_ids` in cards or review links.

Do **not** reset `start_busy` / `qc_busy` in the Effect. An in-flight spawn from the old root still owns those flags until it exits; it must ignore the result when `root_sig` drifted, then set busy false. Clearing busy mid-flight would let the operator double-click while the old wait is still running.

---

## 4. Out of scope (do NOT do here)

- Produce canvas / protocol pane / Stage column (**0125**).
- Process extract-all Busy / orphan rows (**0122**).
- Image-tab overlay / Burn counts (**0120**). OPT QC sniff (**0121**).
- Categorical privilege log (**D-0031-03**). `PRIVILEGE/` folder (**D-0040-04**). Slipsheets (**D-0040-10**).
- Auto-suggest Bates on first empty field (**D-0060-03** remainder).
- Changing `fail_if_withheld` / `require_qc_pass` engine defaults.
- Schema bump, unique-pst, BCC-default, `innerHTML`, daemon.

---

## 5. Preconditions & dependencies

- **P1 (blocking):** 0113 wizard + 0116 `produce_start`/`produce_qc_run` job_id path. `SCHEMA_VERSION` 41. Re-verify at execute.
- **P2:** chrome-ui job still builds trunk + `cargo test -p dedupe-chrome` + ui `Cargo.toml` tests (0118).
- *Verified to date:* §2.2 on HEAD `1b32854`. Last-PR: Bugbot usage-limit only.

---

## 6. Risks

| Risk | Mitigation |
|---|---|
| `Some([])` vs `None` silently empties Desk exports | Tests: `None` still unfiltered; only `Some([])` is 0 |
| Empty-union blank-blocker loses accidental corpus alarm | **Accepted.** QC on empty union counts 0 blanks; `None` remains Desk/CLI corpus path |
| Start `ok: true` confused with volume ok | Volume UI only after `succeeded` |
| Cancel → `paused` already error; `idle` still success | Success = `succeeded` only |
| Matter switch races in-flight wait | Ignore wait result if `root_sig` no longer matches captured root; do not clear busy flags in Effect |
| QC `start_result.set(None)` re-arms Finalize | Disarm reads `volume_succeeded`, not `start_result.ok` |
| Applying `next_seq_hint` overwrites operator edit | Apply after **this** success refresh only, not on every page poll |
| Touching 0125 layout | Fence; keep tabbed steps |
| Privilege-in-set bypass | Do not edit QC extras / `fail_if_withheld` |
| Host apply sequence / `connection()` | Matter methods only |
| Post-step `already open` after 20 retries | Leave silent-Ok; surface `encrypted` / `not_found` only |

---

## 7. Definition of Done

Complete only when ALL hold:

- [ ] **DoD-1 — Empty filter:** `export_privilege_log` + `count_privilege_log_blank_descriptions` with `filter_ids: Some(&[])` return 0 rows / 0 blanks. `None` still unfiltered. Host union `Some([])` does not emit corpus log.
- [ ] **DoD-2 — Second Finalize (primary = disabled):** After `succeeded`, Finalize is **disabled** via `volume_succeeded`. Applying `next_seq_hint` to `bates_start` is required UI affordance, **not** a substitute for the disable. A second click cannot reuse the previous start. Host/UI test or HITL with two clicks. QC re-run after success must **not** re-arm Finalize.
- [ ] **DoD-3 — Matter switch:** Changing `/matters/:id/produce` clears `qc` / `overrides` / `start_result` / `volume_succeeded` / `bates_start` before or with the new page. Does **not** force-clear `start_busy`/`qc_busy`. Review links do not mix old item ids with the new root.
- [ ] **DoD-4 — Cancel / idle:** `wait_process_terminal` success path runs only when `state == "succeeded"`. `cancelled`, `idle`, `paused`, `failed` do not set volume `ok` or load QC findings as a passed run. `process_job_succeeded` tests pass.
- [ ] **DoD-5 — Hygiene:** No `unwrap`/`expect` in new production code. No schema bump. 0113 privilege-in-set tests still pass. `cargo test -p matter-core` privilege tests + `cargo test -p dedupe-chrome` + ui `Cargo.toml` tests + trunk still green.
- [ ] **DoD-6 — Recorded:** `review.md`; registry **Completed**; CHANGELOG Unreleased sentence; `D-0119-produce-checklist-residuals` closed; ledger committed (`BUGFIX`). **0120–0126** stay Proposed unless separately implemented.

---

## 8. Verification commands (reference)

```powershell
Set-Location C:\dev\Dedupe
cargo test -p matter-core --test privilege
cargo test -p dedupe-chrome
cargo test --manifest-path crates/dedupe-chrome/ui/Cargo.toml
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
| **D-0119-produce-checklist-residuals** | **Absorb — this track.** |
| **D-0113-produce-checklist** | Remain closed (**0113** / PR **#117**). |
| **D-0060-03** | **Partial** — apply `next_seq_hint` after success. First-paint auto-fill stays residual. |
| **D-0031-03** | Decline (categorical log). |
| **D-0040-04** | Remain (`PRIVILEGE/` folder). |
| **D-0040-10** | Remain (slipsheets). |
| **D-0120-pdf-raster-ui** | Remain (**0120**). |
| **D-0121-image-opt-qc** | Remain (**0121**). |
| **D-0122-process-fold-residuals** | Remain (**0122**). |
| **D-0125-produce-canvas** | Remain (**0125**). Do not un-wizard here. |
| **D-0020-01** | Decline (operator GUI smoke). |
| **D-0062-codesign** | Remain. |
| Bugbot usage-limit on #125–#128 | **Decline** — not a product finding. |
| BCC-default | Never. |
| Fold-in 2026-08-31 (`opencode-review.md` + `agy-review.md`) | See table below. |

#### Harness fold-in (2026-08-31)

| Id | Disposition |
|---|---|
| opencode-M1 | **Agree — fold.** §3.2 depends on §3.1; dedicated `volume_succeeded` latch; not-succeeded leaves latch; hint only on success refresh. |
| opencode-M2 | **Agree — fold.** Empty-union blank count 0 is accepted (§3.3 + §6). |
| opencode-m1 | **Agree — fold.** Runner pin is `crates/process-runner/src/handlers/produce.rs:93-94`. |
| opencode-m2 | **Agree — fold.** Post-step silent-Ok on persistent `already open`; surface genuine errors only. |
| opencode-m3 | **Agree — fold.** §3.4 does not reset `start_busy`/`qc_busy`. |
| opencode-m4 / m5 | **Decline.** Pin/doctor prose drift; self-corrects. §2.5 updated. |
| opencode-m6 | **Agree — fold.** DoD-2 primary = disabled; hint = required affordance. |
| opencode-O1 | **Already covered.** `review.md` DoD-6. |
| opencode-O2 | **Agree — partial.** Soften `git add -f` (0119 spec/plan already tracked). |
| agy-M1 / M2 / M3 | **Already covered.** §3.3 / §3.1 / §3.2 (volume latch). |
| agy-m1 | **Agree — partial.** Clear session + `bates_start`; do **not** clear busy mid-flight (opencode-m3). |
| agy-m2 | **Already covered** in §6; restated in §3.1 after every await. |
| agy-O1 | **Already covered.** Log error does not re-arm Finalize. |

---

## 10. Unblocks

Counsel can Finalize without colliding Bates, and a skipped-only volume cannot ship a corpus privilege log. **0125** can restyle the canvas on top of this honesty.
