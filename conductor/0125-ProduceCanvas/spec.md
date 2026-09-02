# 0125 — Produce canvas (not a wizard)

> Structure follows `templates/0000-Description/spec.md`. DoD is §7.
> Phase 0 is **closed** in this file. Do **not** steal **0119** latch / empty
> `filter_ids` / cancelled-as-success, **0120** overlay, **0121** OPT skip,
> **0122** Busy, **0124** queue, or **0126** Process jobs table. Do not vendor
> `C:\dev\dedupe-frontend`. Do not mint a BCC-default track. Do not steal
> **0100–0104**. Do not weaken privilege-in-set.

- **Track ID:** 0125-ProduceCanvas
- **Execution repo:** `C:\dev\Dedupe`
- **Governance:** this directory in `C:\dev\Dedupe\conductor\` (track registry: `../conductor.md`)
- **Plan-of-record reference:** Hermes produce checklist. `C:\dev\Dedupe-plan.md` is **absent** (re-verified 2026-09-01); do **not** chase it at execute.
- **Cross-repo contract:** mock at `C:\dev\dedupe-frontend` is **layout research only** (three panes `236px 1fr 320px`). Do not port Archivo, coral, fake `ACME` Bates ranges, unimplemented categorical log, or PDF-as-image.
- **Status:** In progress
- **Depends on:** **0113 / 0115 / 0119 Completed** · **0123 Completed** (PR **#139** / `fce416e`) · **0124 Completed** (PR **#141** / `ff8b0ea`) · schema **v41** (no bump)
- **Spec authored:** 2026-09-01 (placeholder → Ready)
- **Series:** T (mockup chrome fidelity)
>
> **Closes / absorbs:** `D-0125-produce-canvas` (this track). Does **not** close D-0126, D-0031-03, D-0040-04, D-0040-10, D-0060-03, D-0115-lfp, D-0110-deny-unic, D-0062-codesign.
> **HITL:** owner launches the **release** chrome EXE on a synthetic matter: all five steps visible without changing a tab; Stage pane present; Finalize disabled while pre-flight blockers remain; after one successful Finalize, second click stays latched (**0119**). Protocol shows **none on file** when notes are empty. INC* unique-pst is **not** a gate. Codesign is **D-0062-codesign**.
>
> **Harness fold-in (2026-09-01):** `opencode-review.md` + `agy-review.md`. Pad width is an additive `ProductionProfileThin` field from `p.body.bates.pad_width` (not `effective_pad_width` on Thin). QC does **not** auto-run on mount; Pre-flight/Stage say “QC not yet run.” UI DTO new fields get `#[serde(default)]`. See §2.9 / §9.
>
> **Owner locks (inherit 0123):** IBM **Plex**, action **ink-navy `#1b3049`**, cool paper, red = privilege / withhold / blocker only. **0123** 46/30 shell stays. `PRODUCE_FLAG` already ships the privileged-doc rule. This track **fills** the reserved Produce right-slot / status-left — it does not restyle the shell.
>
> **Stack lock:** Tauri **2** + Leptos **0.8 CSR** on **stable** Rust. `ui/` stays workspace-excluded. No daemon. No schema bump. 0113 `fail_if_withheld=true` / `require_qc_pass=true` unchanged. 0119 `volume_succeeded` latch unchanged. Default DAT-only (`us_concordance_native_text_v1`) unchanged. Page-level Bates only when **0115** `us_concordance_image_opt_v1` (or another live `include_images` + `bates_mode=page` profile) is selected.

---

## 1. Objective

Show the whole production decision on **one canvas** so counsel can see set, numbering, format, burn, and pre-flight **before** Finalize — not a wizard that hides QC behind tab 5 while Finalize sits on every step.

Colliding Bates and a corpus-wide privilege log were **0119**. This track is the same honesty class for **layout**: hidden pre-flight is a silent skip of blockers. Unique-export is unchanged.

---

## 2. Context (read before starting)

### 2.1 Why this track, now

**0123 Completed** shipped the shell with Produce flag on the right and empty Produce slots. **0124 Completed** filled Review slots via `QueueChromeCtx` (pattern to copy, not steal). **0119 Completed** (PR **#129** / `6a775b5`) locked Finalize latch. Live produce (`ui/src/pages/produce.rs`) is still **two columns** (`220px 1fr`) with **tabbed** `Show when=step==N` and **Finalize** in `.produce-foot` (full-width footer **below** `.produce-layout`, not inside the left aside). No protocol pane. No Stage column.

### 2.2 Live APIs (plan-time 2026-09-01, HEAD `4edf099`; re-verify at execute)

| Surface | Fact |
|---|---|
| `schema.rs` | `SCHEMA_VERSION == 41`. **No bump.** |
| `ui/styles/app.css` `.produce-layout` | `grid-template-columns: 220px minmax(0, 1fr)`. No third column. |
| `ui/src/pages/produce.rs` | `step: RwSignal<u8>` 1–5; each step wrapped in `Show when=step==N`. `.produce-foot` is a **full-width footer below** the two columns (not in the left aside). Finalize uses `finalize_blocked_by_volume_latch` + Bates ≥ 1 + QC blockers/empty set + warn overrides. `volume_succeeded` latch + `wait_process_terminal` success iff `succeeded`. Default filter copy “Responsive NOT withheld.” `default_count` on Set when not entire-corpus. Prefix + Bates start; Doc/Page Bates toggles **profile**. Format: DAT-only vs TIFF G4+OPT from `include_images`. Categorical radio **disabled** (D-0031-03). Burn selected set requires QC `ordered_ids`. Step-5 tab click **auto-runs** `run_qc` when `qc` is None — that trigger dies with the wizard. Next-seq hint copy hardcodes `"PROD"` while the prefix input is editable. |
| Host `src/produce.rs` | `ProducePageResponse`: sets, `default_count`, `qc_gate`, `next_seq_hint`, `produced_count`, profiles, burn counts, `ordered_ids`. **No** protocol DTO, **no** page-count, **no** slipsheet count, **no** snapshot job, **no** `pad_width` on `ProductionProfileThin` (slug/name/qc_pack_id/include_images/bates_mode only). `produce_start_blocking` always `fail_if_withheld: true`, `require_qc_pass: true`. `get_privilege_protocol` already used for blank-description extras. `list_production_profiles` already has `p.body.bates.pad_width` — not copied onto Thin. |
| `ui/src/invoke.rs` `ProducePageResponse` | Separate WASM mirror. Late fields (`need_burn` / `burned_fresh` / `unmapped_text` / `ordered_ids`) already `#[serde(default)]`. New protocol / pad fields **must** get the same or mount deserializes fail. |
| `ProductionSetThin` | `id`, `name`, `status`, `produced_ok_count`, `bates_prefix`, `next_seq`, `output_root`. **No** Bates range string, **no** blocker count. |
| `PrivilegeProtocol` | `log_format`, `fre_502d_note`, `fre_502e_note`, `description_required`. **No** EDRM category / D. Del. fields. |
| `matter-produce` | `effective_pad_width(&ResolvedProduceConfig)` is a **function after** `resolve_produce_config` (job overlay). Display pad does **not** need that resolve: copy `p.body.bates.pad_width` onto Thin. Job-param pad overlay stays Finalize-only. |
| `ui/src/app.rs` `wrap_produce` | Mounts `<ProducePage/>` inside `MatterShell` with **no** ctx. `ProducePage` owns all signals internally. |
| `ui/src/shell.rs` | `PRODUCE_FLAG` = privileged-doc override sentence. `QueueChromeCtx` is **queue-route only**. Produce right-slot / status-left still empty unless this track provides a **Produce** ctx. |
| Mock `pages/produce.rs` | `.panes-3-produce` `236px 1fr 320px`. Fake VOL001–003, ACME Bates, 8,441 docs / 41,206 pages, Stage & snapshot. Categorical seg. **Do not port fakes.** |
| MS-PST | **N/A this track.** |

### 2.3 Mock + Hermes (research only)

Steal **layout**: three panes always visible; protocol block; Stage 320px; Finalize only in Stage and disabled on blockers. Do not steal mock Page-level-as-default (DAT-only stays default). Do not steal Stage freeze/snapshot (no host job). Do not steal EDRM “B” copy.

### 2.4 Plan-time crate pins (re-verify at execute)

| Crate | Plan-time | Rule |
|---|---|---|
| `tauri` | **2** | Reject 3.x. |
| `leptos` / `leptos_router` | **0.8.x** CSR | No major bump. |
| Schema | **41** | No bump. |
| Rust | **stable** | No nightly. |
| trunk | **0.21.14** | Keep. |
| chrome version | **0.2.0-rc.1** | No bump this track. |

### 2.5 Tools / recall

Ran from `C:\dev\Dedupe`:

- `ai-brains preflight --summary` (inited; plan-time **4605**; fold-in **4606**).
- Recall: 0113 DAT-only + privilege-in-set (`635317a9`); 0119 `volume_succeeded` / do not steal 0125 (`750d2e0c`); 0121 not canvas (`341e108d`).
- `ledgerful doctor --json` `readyForPublish: true`; warns phantom-promote, completion-unreachable, sig-pin, sig-version, impact-stale (refreshed this pass), search-empty.
- Ledger compact: **0 pending / 0 unaudited drift**. Execute starts `0125-produce-canvas` **FEATURE** after owner git-commits this plan if still dirty.
- `ledgerful scan --impact` **LOW** on stray untracked (`.claude`, repo-root `agy-review.md`, `fixtures/keep_set_summary.json`). Do not `git add` those. Federated `output/` 5000-file budget — ignore. Hotspot: `queue.rs` (do not edit). `produce.rs` is the edit surface — keep 0119 `include_str` latch tests green.

### 2.6 What we could not verify

Owner HITL all-five-visible + latch on the release EXE. Execute re-reads `ProducePageResponse` if anyone added protocol fields since this pin.

### 2.7 Related deferred (roll)

See §9. Absorb **D-0125**. Remain D-0126, D-0031-03, D-0040-10, D-0060-03. Decline D-0032-08 / D-0020-01.

### 2.8 Last-PR Cursor comments (2026-09-01)

PRs **#142, #141, #140, #139**. Inline comments **0**. Reviews **0**. Issue comments Bugbot **usage-limit** only. **Decline**.

| Origin | Verdict |
|---|---|
| #141 / #142 ReviewQueueChrome | **Remain 0124** (Completed). Copy `QueueChromeCtx` pattern; do not edit queue. |
| #139 / #140 MatterShell | **Remain 0123**. Fill Produce slots only. |
| #142–#139 usage-limit | **Decline**. |

No new mint. Next free ID **0127**.

### 2.9 Product locks (do not invent at execute)

**Un-wizard (DoD-1 — first).** Render steps 1–5 as **visible panels** on the center pane (mock: 1–3 in a row, then 4 Burn, then 5 Pre-flight filling the rest). Do **not** gate panel bodies on `step == N` so QC is undiscoverable. A step `ol` for scroll-to is OK; if used, give panels `#step-1-set` … `#step-5-preflight` hrefs (no scroll-math). Do **not** rewrite QC / start / latch logic while moving markup.

**QC trigger (un-wizard).** Drop the step-5 tab **auto** `run_qc`. Do **not** auto-run on mount (Busy churn / `busy:` banners). Keep the explicit **Re-run QC** button. Until `qc` is `Some`, Pre-flight **and** Stage show **QC not yet run — click Re-run QC**. Finalize stays disabled while `qc` is `None` (already true).

**0119 latch frozen.** Keep `finalize_blocked_by_volume_latch`, click no-op when `start_busy \|\| volume_succeeded`, `volume_latch_after_produce_terminal`, `process_job_succeeded`, matter-switch session clear, `Some([])` empty-union blank 0. Host `fail_if_withheld` / `require_qc_pass` tests stay. Moving the button into the Stage pane is **not** a license to drop those helpers.

**Privilege-in-set.** Hard block. No chrome bypass. No fake categorical log (**D-0031-03** stays disabled). Default FilterSpec remains responsive AND NOT withheld + include family (entire-corpus checkbox stays withhold=false).

**Three panes.** CSS ~ `236px minmax(0,1fr) 320px` (mock `236px 1fr 320px`). Plex / `#1b3049` / 0-radius **page** rules may match 0123 shell hairlines; do not vendor coral.

**Left — sets + protocol.**

- Keep empty state (`No volumes yet` / produced count). Add **New** = reset **draft**. Handler: `if start_busy.get() || qc_busy.get() { return; }`. Then clear QC, overrides, `start_result`, `volume_succeeded`, `bates_start` to `""` (D-0060-03: no first-paint fill), restore profile to `us_concordance_native_text_v1`. Do **not** insert a fake `VOL003` row.
- When `sets` exist, stack `ProductionSetThin` with **live** fields (`name`, `status`, `produced_ok_count`, `bates_prefix`, `next_seq`). Do **not** invent ACME ranges or blocker counts. Optional: if QC is loaded, show that run’s error/warn counts on the **current draft**, not on historical volumes.
- Protocol block **always** renders. Host: add read-only protocol fields to `produce_page` from `get_privilege_protocol` (JSON additive, no schema bump). **Also** add `pad_width: u32` on `ProductionProfileThin` from `p.body.bates.pad_width` (already loaded; do **not** call `resolve_produce_config` just to display pad). UI `invoke.rs` mirror: same fields with `#[serde(default)]`. Do not mint `upsert` UI. Display `fre_502d_note` / `fre_502e_note` / `log_format`; empty notes → **none on file**. Do **not** invent EDRM category / D. Del. strings. Audit footnote (static): overrides are written to the audit log (already true on Finalize).

**Center — steps (honesty, not mock numbers).**

| Step | Lock |
|---|---|
| 1 Set | Named default search + `default_count` docs. Entire-corpus: honest “count refreshes at QC,” not a fake 0. QC gate line stays. |
| 2 Number | Prefix + Bates start (still required ≥ 1; **D-0060-03** first-paint auto-fill stays residual — apply `next_seq_hint` only after **0119** success). Show **read-only** pad width from the selected Thin `pad_width` (profile body). Hint/projected last Bates use the **live prefix signal**, not hardcoded `"PROD"`. Projected last Bates = doc-level `start + n - 1` when `n` is `qc.ordered_ids.len()` or `page.ordered_ids.len()`; if unknown, omit — do not invent pages. Page-level **only** when the image profile is selected (existing toggle). Families locked on. |
| 3 Format | DAT-only: NATIVES + TEXT + DAT; no IMAGES/OPT. Image profile: TIFF G4 + IMAGE.opt (existing copy). Do **not** enable PDF-as-image (0115 declined). Categorical radio stays disabled. |
| 4 Burn | Keep counts + “Highlights never burn.” Burn selected set still requires QC ids (0120 recount stays). Do not present Burn as a skip-pre-flight primary. |
| 5 Pre-flight | Same extras/findings/override forms. Card/badge restyle OK. Empty set remains a **blocker**, not a log line. |

**Right Stage (320px).** Counts from **live** APIs only:

| Row | Source |
|---|---|
| Documents | `qc.ordered_ids.len()` if QC ran, else `page.ordered_ids.len()` / `default_count` (label which). |
| Pages | `"—"` unless a real page total already exists on the QC/page payload (it does **not**, plan-time). |
| Natives | same as documents for DAT-only; do not invent 612. |
| Slipsheets | `"—"` / 0 + “not this track” (**D-0040-10**). |
| Marks to burn | `need_burn` (page or QC). |
| Withheld | only if an existing extra/finding count exists; else `"—"`. |

Export path list **depends on profile**: DAT-only lists `NATIVES/ · TEXT/ · DATA/load.dat · privilege-log.csv`. Image profile **adds** `IMAGES/ · IMAGE.opt`. Never list LFP (**D-0115-lfp**). Privilege-log radios: Doc-by-doc = live `standard` / `automated_metadata`; Categorical stays disabled.

**Stage & snapshot.** No host snapshot job (plan-time). **Omit** or disabled with “not this track.” Do not fake a freeze.

**Finalize.** Move from `.produce-foot` (full-width footer) into the Stage pane. Same disabled predicate as today + latch. Copy may say **Finalize production**. Blocked-until line names blocker/warn counts from the current QC run, or the QC-not-yet-run sentence when `qc` is `None`.

**Shell.** Filling Produce slots is **optional**. If filled: `wrap_produce` **creates** `ProduceChromeCtx` (label signals only) **above** `MatterShell`, same parent→child pattern as `WrapReview` / `QueueChromeCtx`. `ProducePage` **writes** draft/volume labels via `use_context`; do **not** hoist `qc` / `volume_succeeded` / Finalize. Do **not** put `#queue-goto` on Produce. Do not change `PRODUCE_FLAG`. Keep `shell_source_locks` literals (`id="queue-goto"`, `class="right-slot"`, `class="status-left"`, `REVIEW_FLAG`). No avatar.

**0124 lesson.** Stage/Finalize stay **inside** `ProducePage` (the three-column grid is the page). Only shell **label** slots use context. Do not pass Stage as `MatterShell` children from `wrap_produce`.

---

## 3. In scope

`ui/src/pages/produce.rs`, `ui/styles/app.css` produce layout, small `shell.rs` / `app.rs` Produce ctx + slots (if filling slots: hoist **label** signals into `wrap_produce`), additive `ProducePageResponse` protocol fields + `ProductionProfileThin.pad_width` + chrome-ui invoke types with `#[serde(default)]`. Host tests that already lock `fail_if_withheld` stay. CSS/`include_str` locks for un-wizard + latch.

### 3.1 Un-wizard + protocol + Stage

### 3.2 Move Finalize; keep 0119 helpers

---

## 4. Out of scope (do NOT do here)

- `volume_succeeded` / empty-union / cancelled-wait rewrite (**0119**).
- Overlay coords (**0120**). OPT skip / sniff (**0121**). Process Busy (**0122**). Queue (**0124**). Jobs table (**0126**).
- Slip sheets (**D-0040-10**). Categorical log (**D-0031-03**). LFP (**D-0115-lfp**). `PRIVILEGE/` folder (**D-0040-04**). First-paint Bates auto-fill (**D-0060-03**).
- Stage snapshot job, PDF-as-image, fake Bates, schema bump, BCC-default, Archivo/coral, daemon.

---

## 5. Preconditions & dependencies

- **P1 (blocking):** **0113, 0115, 0119, 0123 Completed**. Schema **41**. **0124 Completed** (shell slots exist).
- *Verified to date:* tabbed `step` Show; two-column CSS; Finalize in `.produce-foot`; protocol unused on the page (HEAD `4edf099`).

---

## 6. Risks

| Risk | Mitigation |
|---|---|
| Un-wizard drops latch tests | Keep helpers; `include_str` still requires latch + click no-op. |
| Un-wizard drops QC auto-run | No mount auto-QC; Re-run button + “QC not yet run” copy. |
| Pad width missing on Thin | Copy `p.body.bates.pad_width` onto Thin; ui `#[serde(default)]`. |
| Protocol fields crash WASM | Host + `invoke.rs` in tandem; `#[serde(default)]`. |
| Mock numbers (pages/slipsheets) | `"—"` / omit unless on the DTO. |
| Stage snapshot invented | Disabled/omit. |
| Protocol invents EDRM | Only `PrivilegeProtocol` fields + “none on file.” |
| Page-level as default | DAT-only remains default; page Bates only on image profile. |
| Shell children cannot see `qc` | Finalize stays in `ProducePage`; ctx only for labels, created in `wrap_produce`. |
| `shell_source_locks` broken | Keep `queue-goto` / slot class / `REVIEW_FLAG` literals. |

---

## 7. Definition of Done

Complete only when ALL hold:

- [ ] **DoD-1 — Un-wizard:** Steps 1–5 visible without changing a tab to discover Pre-flight. CSS three-pane. `include_str` (or ui test) fails if step bodies are still mutually exclusive `Show when=step==N`. No tab auto-`run_qc`. Pre-flight/Stage show **QC not yet run** until Re-run.
- [ ] **DoD-2 — Protocol + sets:** Protocol block always present; empty 502 notes render **none on file**. **New** is busy-guarded and clears `bates_start`. Set rows use live `ProductionSetThin` only. UI DTO new fields `#[serde(default)]`.
- [ ] **DoD-3 — Stage + Finalize:** 320px Stage pane; Finalize lives there; disabled while 0119 latch **or** `qc` None **or** pre-flight blockers **or** missing Bates **or** incomplete warn overrides. Stage snapshot not a fake success. Export paths match the selected profile. Pad width from Thin. Prefix hint uses live prefix. Enter/produce still `fail_if_withheld` / `require_qc_pass`.
- [ ] **DoD-4 — Hygiene:** No `unwrap`/`expect` in new production code. No schema bump. 0119 latch tests + host privilege-in-set / empty-union tests still pass. Plex / `#1b3049` / no Archivo/coral. `cargo test --manifest-path crates/dedupe-chrome/ui/Cargo.toml` + `cargo test -p dedupe-chrome`. trunk chrome-ui builds.
- [ ] **DoD-5 — Recorded:** `review.md`; registry **Completed**; CHANGELOG Unreleased; `D-0125-produce-canvas` closed; ledger committed (`FEATURE`). **0126** stays Proposed unless separately implemented.

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
| **D-0125-produce-canvas** | **Absorb — this track.** |
| **D-0123-matter-shell** | Closed in **0123**. Fill Produce slots only. |
| **D-0124-review-queue-chrome** | Closed in **0124**. Copy ctx pattern; do not edit queue. |
| **D-0126-process-chrome-visual** | Remain (**0126**). |
| **D-0119-produce-checklist-residuals** | Closed in **0119**. Latch frozen. |
| **D-0031-03** | Remain (categorical log disabled). |
| **D-0040-10** | Remain (slipsheets `"—"`). |
| **D-0040-04** | Remain (`PRIVILEGE/` folder). |
| **D-0060-03** | Remain (first-paint Bates auto-fill). |
| **D-0115-lfp** | Remain (do not list LFP). |
| **D-0110-deny-unic** | Remain (upstream). |
| **D-0032-08** / **D-0020-01** | Decline (operator GUI smoke). |
| **D-0062-codesign** | Remain. |
| Bugbot usage-limit on #139–#142 | **Decline**. |
| Mock Page-level default / Stage snapshot / ACME ranges / EDRM B | **Decline**. |
| BCC-default | Never. |
| opencode-M1 | **Partial** — pad on Thin from `p.body.bates.pad_width`; not `resolve_produce_config` for display. |
| opencode-m1 / agy-F-0125-02 | **Fold** — no mount auto-QC; “QC not yet run” + Re-run. |
| opencode-m2 | **Fold** — `wrap_produce` creates ctx; page writes labels; Finalize stays in page. |
| opencode-m3 / agy-F-0125-01 | **Fold** — ui `invoke.rs` `#[serde(default)]` on new fields. |
| opencode-m4 | **Already covered** — FEATURE after owner git-commits Ready docs (gitignore already on main). |
| opencode-O1 | **Fold** — `.produce-foot` is full-width footer, not left aside. |
| opencode-O2 | **Fold** — hint uses live prefix, not `"PROD"`. |
| agy-F-0125-03 | **Fold** — New busy-guard; clear `bates_start` to `""`. |
| agy-F-0125-04 | **Fold** — Phase 3 `shell_source_locks`. |
| agy-F-0125-05 | **Fold** — optional `#step-N` anchors if `ol` jumps. |

---

## 10. Unblocks

Counsel can see blockers and export paths before Finalize. **0126** stays independent Process guts on the 0123 shell.
