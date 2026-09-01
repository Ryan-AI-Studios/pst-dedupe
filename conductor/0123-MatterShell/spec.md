# 0123 — Matter shell (shared TopBar + StatusBar)

> Structure follows `templates/0000-Description/spec.md`. DoD is §7.
> Phase 0 is **closed** in this file. Do **not** steal Bugbot residuals
> **0119–0122**, Review rail/columns (**0124**), Produce canvas (**0125**),
> or Process jobs table (**0126**). Do not vendor `C:\dev\dedupe-frontend`.
> Do not mint a BCC-default track. Do not steal **0100–0104**.

- **Track ID:** 0123-MatterShell
- **Execution repo:** `C:\dev\Dedupe`
- **Governance:** this directory in `C:\dev\Dedupe\conductor\` (track registry: `../conductor.md`)
- **Plan-of-record reference:** Hermes matter chrome. `C:\dev\Dedupe-plan.md` is **absent** (re-verified 2026-09-01); do **not** chase it at execute.
- **Cross-repo contract:** mock at `C:\dev\dedupe-frontend` is **layout research only** (46/30 shell, four tabs, StatusBar flag). Do not copy Archivo, coral `#ec3013`, or brand `DEDUPE / REVIEW`.
- **Status:** Completed
- **Depends on:** **0110–0122 Completed** (0122 PR **#137** / `f1810fe`) · schema **v41** (no bump)
- **Spec authored:** 2026-09-01 (placeholder → Ready)
- **Series:** T (mockup chrome fidelity)
>
> **Closes / absorbs:** `D-0123-matter-shell` (this track). Does **not** close D-0124–D-0126, D-0116-workflow / drop / report, D-0110-deny-unic, D-0062-codesign.
> **HITL:** owner launches the **release** chrome EXE on a synthetic matter: Open lands on **Home under the shared bar** (not a deep-link to Process). Process, Review, Produce, and Admin all show the **same** 46px TopBar (four tabs; Admin inert span) and 30px StatusBar. Produce is no longer `← Matter home` with no tabs. Recents JSON with a UTF-8 BOM loads. INC* unique-pst is **not** a gate. Codesign is **D-0062-codesign**.
>
> **Last-PR fold-in (2026-09-01):** PRs **#138, #137, #136, #135**. Disposition in §2.8. No new mint. Next free ID **0127**.
>
> **Fold-in (2026-09-01):** `opencode-review.md` + `agy-review.md`. OpenCode **M1** / Agy **M2**: restructure `app.rs` so the global `.top-bar` does not stack on matter routes. Agy **M3**: Review tab stays active on `/review/:docId`. See §9 + `foldin-note.md`.
>
> **Owner locks (2026-09-01, already recorded — do not re-ask):** IBM **Plex** (not Archivo). Action/selection **ink-navy `#1b3049`**. Cool paper ground. Red = privilege / withhold / blocker / draft overlay only — **not** coral. Home stays a workspace route **under** TopBar+StatusBar after Open. Matters list remains the launcher. Privilege first-pass column stays 0111 **PRIV** coding (0124).
>
> **Stack lock (inherit 0110–0122):** Tauri **2** + Leptos **0.8 CSR** on **stable** Rust. `ui/` stays workspace-excluded. One pipeline (`process-runner`). No daemon. No schema bump. 0119 `volume_succeeded` unchanged. 0121 OPT/sniff unchanged. 0122 extract-all Busy / `busy_retry_pending` / live row Pause unchanged. Default DAT-only and `qc_default_v1` unchanged.

---

## 1. Objective

Once a matter is open, every workspace route uses the **same** mockup shell: 46px top bar (brand · matter name · Process/Review/Produce/Admin · right slot) and a 30px status bar. Admin stays an **inert tab label**, not a blank page without chrome. Home stays under that bar. Matters list stays the launcher.

This is **orientation honesty**: coding the wrong workspace because Produce dropped the tabs is the same class as a silent unique-export drop. Unique-export itself is unchanged.

---

## 2. Context (read before starting)

### 2.1 Why this track, now

Series O chrome (**0110–0122**) shipped the workspaces. Series T steals **layout** from the mock so counsel can see which workspace they are in. **0122 Completed** (PR **#137** / `f1810fe`) — do not retouch extract-all Busy. This track is the Series T spine: **0124–0126** hang their page chrome on this shell.

### 2.2 Live APIs (plan-time 2026-09-01, HEAD `ac9a99d`; re-verify at execute)

| Surface | Fact |
|---|---|
| `crates/matter-core/src/schema.rs` | `SCHEMA_VERSION == 41`. **No schema bump.** |
| `ui/src/app.rs` | One dark `.top-bar` for **all** routes (`Dedupe Desk` + static hint). Routes: `/matters`, `/matters/:id`, process, review, `review/:docId`, produce, admin. **No** nested `Outlet`. Ctrl+K focuses `#matter-search` on the list; elsewhere a visible no-op hint. |
| `pages/home.rs` | Overview chips + CTAs + **in-page** five-tab nav (Home is a tab). Placeholder empty copy. `← Matters`. |
| `pages/process.rs` / `queue.rs` | In-page five-tab nav + `← Matter home`. Process body still has `STATUS_BAR` (“Processing is deterministic…”) at ~833. Host test `process_ui_is_live_not_stub` (`src/process.rs` ~830) `include_str!`s that sentence — **keep the sentence in-tree**; update the test path if it moves to StatusBar. |
| `pages/produce.rs` ~590 / `admin.rs` / `review_window.rs` ~1080 | **No** workspace tabs. Only `← Matter home` (window also has `← Queue`). |
| `src/recents.rs` | `serde_json::from_str(&raw)` — a UTF-8 BOM (`\u{feff}` / `EF BB BF`) fails `expected value at line 1 column 1`. Writes via `to_string_pretty` (no BOM today unless the file was hand-edited). Tests cover MRU/20, not BOM. |
| Mock `top_bar.rs` | Four tabs; **Admin `href: None`** → `<span>`. Brand `DEDUPE / REVIEW`. Right slot = `children`. |
| Mock `app.css` | `.app-shell` `grid-template-rows: 46px 1fr 30px`. `.topbar` / `.statusbar` 2px ink rules, radius 0. |
| Mock tokens | Archivo + coral — **do not port**. `--color-ink: #1b3049` is the action navy to steal. |
| Live `ui/styles/tokens.css` | Plex already. `--chrome-bg: #0f1419`, `--radius-control: 4px`, `--ink: #1a1f24` (body). Shell must not stay the dark global header on matter routes. |
| MS-PST | **N/A this track.** |

### 2.3 Mock + Hermes (research only)

Steal **structure**: 46/30, four workflow tabs, StatusBar flag on the right, 0-radius 2px ink on the **shell**. Do not vendor coral, Archivo, or `DEDUPE / REVIEW`. Process jobs table / drop copy = **0126**. Review rail / column ellipsis / Go-to = **0124**. Produce five-step canvas = **0125**.

### 2.4 Plan-time crate pins (re-verify at execute)

| Crate | Plan-time | Rule |
|---|---|---|
| `tauri` | **2** | Reject **3.x / pre-release**. |
| `leptos` / `leptos_router` | **0.8.x** CSR | Do not bump major. Nested `Outlet` is allowed if it compiles on 0.8; otherwise compose the same `MatterShell` in each page. |
| Schema | **41** | No bump. |
| Rust | **stable** (CI) | No nightly. |
| trunk | **0.21.14** (ci.yml) | Keep. |

### 2.5 Tools / recall

Ran from `C:\dev\Dedupe`:

- `ai-brains preflight --summary` (inited; **4596** pinned at fold-in 2026-09-01; plan-time was 4178).
- Recall: owner 0123 locks (`b2f60ecd`) — Plex, `#1b3049`, Home under bar, no Archivo/coral. 0122 Completed (`3783d9de`) — do not steal extract-all.
- `ledgerful doctor --json` `readyForPublish: true`; warns phantom-promote, completion-unreachable, sig-pin, sig-version (impact-stale was transient).
- Ledger compact: **0 pending / 0 unaudited drift**. Planning FEATURE tx `2e1b45ab` already committed. Execute: owner git-commits conductor/docs first, then a **new** `0123-matter-shell` **FEATURE** for product code.
- `ledgerful scan --impact` **LOW** on stray untracked (`.claude`, repo-root `agy-review.md`, `fixtures/keep_set_summary.json`). Do not `git add` those. Federated scan hit the 5000-file budget under `output/` — ignore.

### 2.6 What we could not verify

Owner HITL on the release EXE (Produce tabs, Home-under-bar, BOM recents). Execute re-reads leptos_router 0.8 nested-route/`Outlet` if used.

### 2.7 Related deferred (roll)

See §9. Absorb **D-0123**. Remain D-0124–D-0126, D-0116-*. Decline D-0032-08 / D-0020-01 as operator smoke.

### 2.8 Last-PR Cursor comments (2026-09-01)

PRs **#138, #137, #136, #135**. Inline review comments **0**. Issue comments are Bugbot **usage-limit** only. **Decline** as product input.

| Origin | Verdict |
|---|---|
| #137 Process extract-all / row Pause | **Remain 0122** (Completed). Do not steal. |
| #138 docs 0122 Completed | **Decline** (docs). |
| #135 / #136 image OPT QC | **Remain 0121** (Completed). |
| #138–#135 usage-limit | **Decline**. |

No new mint. Next free ID **0127**.

### 2.9 Product locks (do not invent at execute)

**Shell geometry.** Matter routes use a three-row grid: **46px** TopBar · **1fr** content · **30px** StatusBar. Shell chrome (topbar, tabs, statusbar, pane hairlines) is **radius 0** and **2px ink** rules. Cool paper ground stays (`--surface` `#f6f4ef`). StatusBar is **paper ground, 2px ink rule, dark `--ink` text** — do **not** adopt the mock’s dark strip (`background: var(--color-text)`). Body text ink may stay `#1a1f24`. Action/selection (active tab, primary) uses **`#1b3049`**. Do not restyle every card/button this track (0124–0126 own page guts).

**`app.rs` — no stacked headers.** Live `<header class="top-bar">` sits **outside** `<Router>` (`app.rs` ~130 vs ~144), so composing `MatterShell` inside pages **without** this step ships two bars (DoD-1 fail). **Required:** move chrome **inside** `<Router>` (nested parent+`Outlet`, or a route-conditional header that can see the location). Launcher `.top-bar` (list hint / Ctrl+K search) renders **only** on `/matters`. Matter routes render **only** the 46px matter TopBar. Keep skip-links and `#main-content`. Keep `#ctrl-k-hint` on a **persistently mounted** node (app-shell sibling, not only the list header) so matter-route Ctrl+K still shows the existing visible no-op. `use_location` only works inside `<Router>` — do not try to hide the pre-Router header from outside.

**Brand.** Keep **`Dedupe Desk`**. Do **not** relabel to mock `DEDUPE / REVIEW`.

**Four tabs, Home is not a fifth tab.** Tabs are **Process · Review · Produce · Admin** only (mock `Tab` enum). Hrefs are live routes: `format!("/matters/{encoded_id}/process")` (and `/review`, `/produce`) — **never** copy mock root-relative `"/process"`. **Home** is brand click **and** matter-name click → `/matters/:id`. On the Home route, **no** workflow tab is `active`; matter name is the Home affordance (`aria-current` on that link). **Active tab is explicit** from the wrapping route (`Tab::Review` on both queue and `review/:docId`), not `pathname == "…/review"` exact equality — the document window must keep Review highlighted. Remove in-page `<nav class="tabs">` from home/process/queue once TopBar owns them. Remove `← Matter home` from matter pages (brand/name is the way Home). Keep **`← Matters`** only on the launcher or as a small TopBar control that leaves the matter shell (list is outside the shell).

**Admin.** Inert **`<span>`**, not an `<A>` or button (mock `href: None`). No hover-as-link styling. Do not invent Admin features. Route `/matters/:id/admin` may remain: same shell, one honest sentence in the body (“Admin is a later design batch”), not a toolbar-only stub without tabs. Clicking the Admin span does **not** navigate.

**Matter name / meta.** Only `home.rs` invokes `matter_overview` today. **`MatterShell` fetches once** when `:id` changes (shared context or parent). Home chips read that same overview — do not spawn five divergent per-page invokes. On invoke **fail**: TopBar name = last segment of the decoded matter root (or `"Matter"`); **omit** processed/meta (do not invent `0`). Surface the invoke error on the page as today.

**Review window.** **Same TopBar + StatusBar** (orientation: counsel can switch workspace). Do **not** restyle the three-pane coding surface. Keep **`← Queue`**. Do not put 0124 Go-to in the right slot this track — **reserve** the slot (empty or a disabled placeholder). Right-slot container is flex and **collapses when empty** so tabs do not jump. StatusBar left on the window may stay empty; do not steal 0118 fetch guards.

**StatusBar flags (right `.flag`).**

| Route | Right flag (this track) | Left slot |
|---|---|---|
| Process | Move `STATUS_BAR` here; **remove** it from the Process page body. Host `process_ui_is_live_not_stub` must still see the sentence (update `include_str` to the shell file if needed). | Reserved for job % (**0126** may fill). Empty OK. |
| Review queue | Short first-pass rule, e.g. privilege column is coding (`PRIV`), not withhold. | Reserved for **0124** `Rows a–b of N`. Empty OK. |
| Review window | Same as queue or “Save & Next codes this document.” | Empty OK. |
| Produce | 0113 honesty: a privileged document cannot enter a production set without a documented override. | Reserved for VOL status (**0125**). Empty OK. |
| Home | Optional one-liner (overview is in the page). | Empty. |
| Admin | “Admin is a later design batch.” | Empty. |

**Right TopBar slot.** Per-page children. Flex container that **collapses when empty** so tabs stay put. Process **may** pass existing `progress` kind/state if cheap (no new host API). Review Go-to is **0124**. **No avatar.**

**Matters list.** **Outside** the matter shell. Keep Ctrl+K → `#matter-search`. Do not put Process/Review/Produce/Admin tabs on the list. Launcher header may stay the simpler list chrome (not the 46px matter TopBar).

**Home body.** Keep overview chips + CTAs. **Delete** the placeholder empty sentence as the only extra body. Do **not** deep-link Open to Process/Review.

**Recents BOM.** On read, strip a leading UTF-8 BOM (`\u{feff}`) before `from_str`. Writes stay BOM-less. Unit tests: BOM file loads; written bytes do not start with `EF BB BF`. Do not silently drop a corrupt file that still fails after strip.

**Do not change** `process-runner` Busy, 0122 extract-all / `busy_retry_pending` / `is_orphan_running`, 0119 produce waits, 0121 QC, queue virtualization math (0117), or review_window apply sequence (0118).

---

## 3. In scope

`dedupe-chrome` UI shell + `recents.rs` BOM. CSS tokens for shell geometry / navy / 0-radius shell rules. Shared `TopBar` / `StatusBar` / `MatterShell` (new ui modules). Host recents tests + update `process_ui_is_live_not_stub` if the deterministic sentence moves.

### 3.1 Shared matter chrome

Matter routes (Home, Process, Review, Review window, Produce, Admin) share one TopBar + StatusBar. Nested parent+`Outlet` **or** the same components composed in each page — both OK; forgetting Produce/Admin is a fail. **`app.rs` must be edited** so the pre-Router global header does not remain on matter routes (§2.9).

### 3.2 Recents BOM

`strip_utf8_bom` (or equivalent) in `recents.rs`; tests in that file.

---

## 4. Out of scope (do NOT do here)

- Review 244px rail, column ellipsis / collision, Go-to Control#/Bates/subject, row-range text (**0124**). Reserve the slots only.
- Produce five-step un-wizard, protocol pane, Stage vs Finalize (**0125**). Privilege-in-set **logic** stays 0113/0119; only the StatusBar **flag** copy ships here.
- Process jobs table, locked-profile checklist, drop copy, `\\?\` strip, minus-stack, report download (**0126** / D-0116-*).
- 0122 extract-all Busy / live orphan Pause.
- Queue viewport height after the 46+30 shell (0117 is measurement-based; **0124** row-range uses whatever remains).
- 0119 cancelled-produce latch.
- Archivo, coral, `DEDUPE / REVIEW`, avatars, Admin product UI.
- Schema bump, BCC-default, WASM jobs, deleting Desk Process, daemon.

---

## 5. Preconditions & dependencies

- **P1 (blocking):** **0110–0122 Completed**. Schema **41**.
- *Verified to date:* Produce/Admin lack tabs; recents has no BOM strip; Home has in-page five tabs; Process STATUS_BAR is still body copy (HEAD `ac9a99d`).

---

## 6. Risks

| Risk | Mitigation |
|---|---|
| Nested `Outlet` surprises on leptos 0.8 | Compose `MatterShell` in each page if Outlet is awkward; DoD is visual/shared, not the routing primitive. **Still edit `app.rs`.** |
| Host test fails when STATUS_BAR leaves `process.rs` | Update `process_ui_is_live_not_stub` to the new file; do not drop the sentence. |
| Admin span vs leftover `/admin` bookmarks | Same shell + one sentence; span does not navigate. |
| Dark global header remains on matter routes | **Required `app.rs` step:** header inside `<Router>` or route-conditional; list-only launcher bar; `#ctrl-k-hint` stays mounted. |
| Review tab goes idle on `/review/:docId` | Pass `Tab::Review` from the window route (same as queue). |
| Five pages each invent a TopBar name fallback | One `matter_overview` in `MatterShell`; fail omits processed/meta. |
| 0122 extract-all regressions while editing `process.rs` toolbar | Touch only the toolbar/tabs/STATUS_BAR move; do not edit `busy_retry_pending` / drain. |

---

## 7. Definition of Done

Complete only when ALL hold:

- [x] **DoD-1 — Shared TopBar:** Home, Process, Review queue, Review window, Produce, and Admin all show **exactly one** 46px TopBar (no stacked `app.rs` global header): brand `Dedupe Desk`, matter name (from shared `matter_overview`) + processed/meta, four tabs (Admin inert span). Home is brand/name, not a fifth tab. **Review stays `active` on `/review/:docId`.** Produce is not `← Matter home` alone. In-page workspace `<nav class="tabs">` is gone from those pages.
- [x] **DoD-2 — StatusBar:** 30px bar on those routes. Process deterministic sentence is the **flag**, not body copy. Produce flag is the privileged-document override rule. Host `process_ui_is_live_not_stub` still passes.
- [x] **DoD-3 — Home + list:** After Open, Home is under the shell with overview chips (no placeholder-only body). Matters list has **no** workflow tabs. No deep-link Open → Process.
- [x] **DoD-4 — Recents BOM:** UTF-8 BOM file loads; writes are BOM-less. Unit tests in `recents.rs`.
- [x] **DoD-5 — Hygiene:** No `unwrap`/`expect` in new production code. No schema bump. 0122 Busy tests still pass. Tokens: Plex, `#1b3049` action, no Archivo/coral. `cargo test --manifest-path crates/dedupe-chrome/ui/Cargo.toml` + `cargo test -p dedupe-chrome`. trunk chrome-ui still builds.
- [x] **DoD-6 — Recorded:** `review.md`; registry **Completed**; CHANGELOG Unreleased sentence; `D-0123-matter-shell` closed; ledger committed (`FEATURE`). **0124–0126** stay Proposed unless separately implemented.

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
| **D-0123-matter-shell** | **Absorb — this track.** |
| **D-0124-review-queue-chrome** | Remain (**0124**). Go-to slot reserved, not filled. |
| **D-0125-produce-canvas** | Remain (**0125**). StatusBar Produce **flag** ships here; canvas does not. |
| **D-0126-process-chrome-visual** | Remain (**0126**). Process flag ships here; jobs table does not. |
| **D-0122-process-fold-residuals** | Closed in **0122**. Do not reopen. |
| **D-0116-workflow** / **drop** / **report** | Remain (0126 / later). |
| **D-0110-deny-unic** | Remain (upstream Tauri). |
| **D-0119-produce-checklist-residuals** | Closed in **0119**. |
| **D-0032-08** | Decline (operator GUI smoke). |
| **D-0020-01** | Decline (operator GUI smoke). |
| **D-0062-codesign** | Remain. |
| Bugbot usage-limit on #135–#138 | **Decline**. |
| Archivo / coral / `DEDUPE / REVIEW` | **Decline** (owner 2026-09-01). |
| BCC-default | Never. |
| OpenCode M1 / Agy M2 (stacked `app.rs` header) | **Agree — fold** — required `app.rs` restructure; Phase 2 item. |
| Agy M1 (host STATUS_BAR `include_str`) | **Already covered** — DoD-2 / Phase 2. |
| Agy M3 (Review tab on `:docId`) | **Agree — fold** — explicit `Tab::Review` on the window. |
| Agy m1 (BOM `strip_prefix`) | **Already covered** — Phase 1; corrupt-after-strip still errors. |
| Agy m2 (Admin span not a link) | **Already covered** — plus no hover-as-link. |
| Agy O1 (empty right slot) | **Agree — fold** — flex, collapse when empty. |
| OpenCode m1 (`matter_overview` only on Home) | **Agree — fold** — one shell fetch; omit meta on fail. |
| OpenCode m2 (pin 4596 / uncommitted docs) | **Agree — fold** — §2.5 refreshed; owner git-commit before product FEATURE. |
| OpenCode O1 (StatusBar dark mock strip) | **Agree — fold** — paper + ink rule + dark text. |
| OpenCode O2 (mock `/process` hrefs) | **Agree — fold** — live `/matters/{id}/…` only. |
| OpenCode O3 (76px queue viewport) | **Decline** — 0117 measurement-based; 0124 consumes remaining height. |

---

## 10. Unblocks

Counsel can see Process / Review / Produce / Admin on every matter screen, including Produce. **0124** can put Go-to and row range in reserved slots. **0125** / **0126** restyle page guts on top of this shell.
