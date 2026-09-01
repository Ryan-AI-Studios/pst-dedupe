# 0120 — Pdf-raster UI residuals (PR #119 Bugbot)

> Structure follows `templates/0000-Description/spec.md`. DoD is §7.
> Phase 0 is **closed** in this file. Do not re-open unique-export, matter-home
> (**0110**), first-pass queue (**0111** / **0117**), review-window async
> (**0118**), DAT produce wizard honesty (**0113** / **0119**), zpdf burn
> compose (**0114**), TIFF/OPT QC (**0115** / **0121**), Process extract-all
> (**0116** / **0122**), or produce **canvas** layout (**0125**). Do not vendor
> `C:\dev\dedupe-frontend`. Do not mint a BCC-default track.

- **Track ID:** 0120-PdfRasterUiResiduals
- **Execution repo:** `C:\dev\Dedupe`
- **Governance:** this directory in `C:\dev\Dedupe\conductor\` (track registry: `../conductor.md`)
- **Plan-of-record reference:** Hermes Image tab + produce Burn. `C:\dev\Dedupe-plan.md` is **absent** (re-verified 2026-09-01); do **not** chase it at execute.
- **Cross-repo contract:** mock at `C:\dev\dedupe-frontend` is research only (layout is **0125**; shell tokens are **0123**).
- **Status:** Completed
- **Depends on:** **0114 Completed** (PR **#119** / `5ed53bf`) · **0118 Completed** (raster `generation` already copied) · **0119 Completed** (do not touch `volume_succeeded`) · schema **v41** (no bump)
- **Spec authored:** 2026-09-01 (placeholder → Ready)
- **Series:** O (Review chrome) — PR #119 Image-tab / Burn-count residual
>
> **Closes / absorbs:** `D-0120-pdf-raster-ui` (this track). Does **not** close D-0121–D-0126, D-0114-pdfium-sidecar, D-0114-xform-text, D-0034-05, D-0032-07 inverse, D-0020-01, D-0062-codesign.
> **HITL:** owner launches the **release** chrome EXE, synthetic PDF with a visible token: draw a box that **releases over an existing overlay** → Burn must excise the dragged token, not a neighbor. Change page mid-drag → no geom from the old origin. Burn-step Need burn must drop after a successful set burn. INC* unique-pst is **not** a gate. Codesign is **D-0062-codesign**.
>
> **Last-PR fold-in (2026-09-01):** PRs **#130, #129, #128, #127**. Disposition in §2.8. No new mint. Next free ID **0127**.
>
> **Harness fold-in (2026-09-01):** `opencode-review.md` + `agy-review.md`. Centerpiece: 2 **raster-px** min-drag guard; pane-leave clear is its own Effect; recount the cloned burned-id list (`None`/empty skip); keep `mouseleave` not `mouseout`. Status stays **Ready — not started**.
>
> **Stack lock (inherit 0110–0119):** Tauri **2** + Leptos **0.8 CSR** on **stable** Rust. Plex / paper / cool chrome. Red = privilege / withhold / blocker / **draft redact overlay** only. No daemon. No schema bump. `ui/` stays workspace-excluded. One pipeline. 0114 `redact_page` → incremental write → `rewrite_pdf` unchanged. 0119 `volume_succeeded` unchanged.

---

## 1. Objective

Keep the **0114** Image-tab draw tool and the produce Burn-step **honest**: overlay mouseup must record boxes in **image-frame** space so burn excises the visible drag; in-flight draw state must not survive page/doc/pane change; Need-burn / Burned-fresh must reflect the set that was just burned.

This is **correctness**. A misplaced geom burns the wrong pixels — the same honesty class as a silent unique-export drop. Unique-export itself is unchanged.

---

## 2. Context (read before starting)

### 2.1 Why this track, now

**0114 Completed** (PR **#119**) shipped zpdf CPU raster + geometric burn. Three **valid** Cursor Bugbot findings were parked here so **0115** could proceed. **0118** copied the raster `generation` pattern onto document/body fetches and fenced Image-tab draw. **0119 Completed** (PR **#129**) — do not retouch Finalize latch / privilege-log filter. **0125** owns un-wizard layout — not this ID.

### 2.2 Live APIs (plan-time 2026-09-01, HEAD `e3f4fab`; re-verify at execute)

| Surface | Fact |
|---|---|
| `crates/matter-core/src/schema.rs` | `SCHEMA_VERSION == 41`. **No schema bump this track.** |
| `ui/src/pages/review_window.rs` `css_point_to_raster` (~142–154) | Maps CSS pixels on the displayed `<img>` to raster pixels. Unit-tested. Keep. |
| `event_to_raster` (~156–168) | Uses `current_target` for `client_width`/`client_height` (the `image-frame`) but `ev.offset_x()`/`offset_y()` which [MDN](https://developer.mozilla.org/en-US/docs/Web/API/MouseEvent/offsetX) defines relative to the **event target** padding edge — the child overlay when release happens over `.geom-overlay`. **Still the High bug.** |
| Image-frame handlers (~1290–1318) | `mousedown`/`mousemove`/`mouseup` all store `offset_x`/`offset_y`. Overlay `mousedown` `stop_propagation` (select, not draw). `.geom-overlay` has `pointer-events: auto` (`app.css` ~740); `.draft` is `none`. |
| Doc Effect (~490–498) | On `doc_id` change: resets `raster_page_index`, clears `raster`/`geoms`/`selected_geom`. **Does not** clear `drawing` / `drag_origin` / `drag_now`. |
| Raster Effect (~500–513) | On root/`doc_id`/pane/`raster_page_index`: bumps `raster_generation`, clears raster/geoms. **Does not** clear draw signals. Escape (~844–848) does. |
| `ui/src/pages/produce.rs` Burn step (~783–847) | Counts prefer `qc.need_burn` over `page.need_burn` (~789–803). After `produce_burn_set` Ok, handler refreshes `page` only (~835–841). `qc` counts stay pre-burn. |
| Burn button (~810–822) | Uses `qc.ordered_ids`. Empty → error, no burn. |
| `produce_page_blocking` | `need_burn`/`burned_fresh`/`unmapped_text` from `burn_counts_for_ids` on **default-filter** `ordered_ids` — may differ from the QC set. |
| `ProduceBurnSetResponse` (`raster.rs` ~523–527) | `burned` / `skipped` / `errors` only. No recount. |
| `burn_counts_for_ids` (`raster.rs` ~575–598) | Set-scoped; reuse after the burn loop. |
| `volume_succeeded` (0119, produce.rs) | Finalize latch. **Do not** edit. |
| `fetch_is_current` / `raster_generation` (0118) | Document/body/raster stale-fetch guards. **Do not** rewrite. Geom upsert after mouseup already checks `item_id` + generation on the list refresh. |
| MS-PST | **N/A this track.** |
| `zpdf` | **0.13.0** in `pdf-raster`. Do not bump. Do not change burn compose. |

### 2.3 Mock + Hermes (research only)

Hermes Image tab: draw + burn. Produce Burn step: counts before Finalize. Do not steal 0125 canvas or 0123 shell. Overlay hatch stays red (privilege/redact).

### 2.4 Plan-time crate pins (re-verify at execute)

| Crate | Plan-time | Rule |
|---|---|---|
| `tauri` | **2** | Reject **3.x / pre-release**. |
| `leptos` | **0.8.x** CSR | Do not bump major. |
| `zpdf` | **0.13.0** | Inherit; no compose change. |
| Schema | **41** | No bump. |
| Rust | **stable** (CI) | No nightly. |
| trunk | **0.21.14** (ci.yml) | Keep. |

### 2.5 Tools / recall

Ran from `C:\dev\Dedupe`:

- `ai-brains preflight --summary` (inited; **4172** pinned at fold-in; plan-time recorded 4170 — cosmetic +2).
- Recall: 0114 draft overlays until produce; Highlights never burn; `redact_page` then rewrite; 0118 do not change 0120 overlay; 0115 minted these three UI items here.
- `ledgerful doctor --json` `readyForPublish: true`; scan `--impact` **LOW**. Doctor warns: phantom-promote, sig-pin, sig-version, completion-unreachable — none block. `"impact-stale"` is not currently emitted.
- Ledger compact: 0 pending / 0 unaudited drift at fold-in.

### 2.6 What we could not verify

Owner HITL: mouseup over a live overlay on the release EXE. Execute re-reads `offset_x` vs `get_bounding_client_rect` on wasm `web_sys`.

### 2.7 Related deferred (roll)

See §9. Absorb **D-0120**. Remain D-0114-pdfium-sidecar / D-0114-xform-text / D-0034-05 / D-0032-07. Decline D-0032-08 / D-0020-01 as operator smoke.

### 2.8 Last-PR Cursor comments (2026-09-01)

PRs **#130, #129, #128, #127**. Inline review comments empty. Issue comments are Bugbot **usage-limit** only (no findings). **Decline** as product input. Origin PR **#119** still has the three items this track owns (live-verified at §2.2; PR-branch line numbers drifted).

No new mint. Next free ID **0127**.

### 2.9 Product locks (do not invent at execute)

- Frame-relative coords: `clientX/Y − getBoundingClientRect()` of **`image-frame` (`current_target`)**. Do **not** use `offsetX`/`offsetY` for draw. Do **not** set committed overlays to `pointer-events: none` (select must still work).
- Clear in-flight draw on `doc_id`, `raster_page_index`, pane ≠ `image`, and `mouseleave` of the frame. Escape already clears — keep.
- Burn-step counts after burn are **the burned set** (`qc.ordered_ids`), not “prefer page because page happened to refresh.” Extend `ProduceBurnSetResponse` with `need_burn` / `burned_fresh` / `unmapped_text` from `burn_counts_for_ids` **after** the loop; UI patches those three fields on `qc`. Keep the existing `produce_page` refresh. Do **not** re-run the QC job as the only path. Do **not** clear entire `qc` (0119 ordered_ids / findings / latch).
- Do not change 0114 burn compose, Highlights-never-burn, privilege-in-set, `fail_if_withheld`, `require_qc_pass`, or 0119 `volume_succeeded`.
- Do not rewrite the five-step wizard into 0125.

---

## 3. In scope

UI draw coordinates + draw-state cancel + Burn-step recount. Host DTO only as needed for the recount. **Do not** change `pdf-raster` burn compose or geom user-space mapping.

### 3.1 Mouseup / move / down in image-frame space

Shared helper (ui crate, unit-tested; wasm wrapper calls it):

```text
fn frame_css_point(client_x, client_y, rect_left, rect_top) -> (css_x, css_y)
  // css = client − rect origin (viewport vs getBoundingClientRect)
```

`event_to_raster` (or replacement): `current_target` as `Element` → `get_bounding_client_rect()` + `client_x`/`client_y` → `frame_css_point` → `css_point_to_raster` using **rect width/height** (same box as the subtraction).

Use that helper for **mousedown, mousemove, and mouseup** on `.image-frame`. Draft overlay px stays frame-relative. Clamp CSS points to `[0, rect.width] × [0, rect.height]` before mapping (edge events can sit a pixel outside the box).

Keep the existing **2 raster-px** min-drag guard (review_window.rs:1323) unchanged — `pw`/`ph` are post-`css_point_to_raster`, **not** CSS px. Do not “correct” it to CSS before mapping. Keep generation check on the post-upsert list refresh.

### 3.2 In-flight draw does not survive navigation

Clear `drawing`, `drag_origin`, `drag_now` when any of:

- `doc_id` changes (doc Effect ~490)
- `raster_page_index` changes (inside the raster Effect **after** it is going to load — not after the early-return)
- `pane` leaves `"image"` — **own Effect on `pane`**. The raster Effect early-returns when `pane != "image"` (review_window.rs:505–507), so a clear placed after that return never runs.
- `mouseleave` on `.image-frame` (not `mouseout` — `mouseout` fires on entry to overlay children and would cancel a drag over a committed box)

A mouseup after that clear is a no-op (`if !drawing.get() { return }`). Do not insert a geom from the previous page’s origin.

### 3.3 Burn-step counts follow the burned set

After `produce_burn_set_blocking` finishes the id loop (including partial errors):

- Clone the **concrete** id list **before** the loop (`for id in ids` moves the `Vec`; raster.rs:533–537). Recount `burn_counts_for_ids` on that clone.
- `None` or empty ids → **skip** the recount; response count fields stay `0` (nothing burned). Live UI always sends `Some(qc.ordered_ids)` (produce.rs:812–828).
- Return the three counts on `ProduceBurnSetResponse` (and the ui `ProduceBurnSet` DTO).

UI on Ok:

1. If `qc` is `Some`, patch `need_burn` / `burned_fresh` / `unmapped_text` from the response (clone the struct; do not drop findings / `ordered_ids`).
2. Keep the existing `produce_page` refresh.
3. Show errors joined as today.

Burn-step display may keep preferring `qc` once those fields are patched. DoD is: after a successful set burn that leaves no remaining required items in **that** set, Need burn shows **0**.

Do **not** re-run `produce_qc_run` solely to refresh counts.

---

## 4. Out of scope (do NOT do here)

- TIFF G4 / OPT / page-level Bates / JPEG sniff (**0115** / **0121**).
- Document/body stale fetch, `fetch_is_current`, catalog gate (**0118**).
- Finalize latch / empty privilege-log filter / matter QC reset (**0119**).
- Process extract-all Busy / orphan rows (**0122**).
- Produce canvas / protocol pane / Stage (**0125**). Matter shell (**0123**).
- 0114 burn compose (`redact_page` → write → `rewrite_pdf`). `iw.document()` still forbidden.
- Form XObject under-redact (**D-0114-xform-text**). pdfium sidecar (**D-0114-pdfium-sidecar**). Acrobat viewer (**D-0034-05**). Inverse redact (**D-0032-07**).
- Schema bump, unique-pst, BCC-default, `innerHTML`, daemon.

---

## 5. Preconditions & dependencies

- **P1 (blocking):** 0114 Image tab + `produce_burn_set` + `burn_counts_for_ids`. `SCHEMA_VERSION` 41. Re-verify at execute.
- **P2:** chrome-ui still builds trunk + `cargo test -p dedupe-chrome` + ui `Cargo.toml` tests.
- *Verified to date:* §2.2 on HEAD `e3f4fab`. Last-PR: Bugbot usage-limit only.

---

## 6. Risks

| Risk | Mitigation |
|---|---|
| `offsetX` still used in a fallback | Delete offset path from draw helpers; tests on `frame_css_point` |
| `getBoundingClientRect` vs `clientWidth` mismatch | Use the same rect for origin **and** size |
| Overlay `pointer-events: none` while drawing | Forbidden — breaks select; coords fix is enough |
| Preferring `page` counts after burn | Page is default-filter; lock is burned-set recount on the DTO |
| Patching `qc` drops findings | Patch three u64 fields only |
| Touching 0119 latch / 0118 fetch guards | Fence; do not edit those signals/helpers |
| Touching 0114 compose | Fence `pdf-raster` burn functions |
| Geom upsert after page change | Clear draw first; existing generation check on list refresh |
| Pane-leave clear inside raster Effect | Own Effect on `pane`; raster Effect returns early off-image |
| `mouseout` cancels drag over overlays | Use `mouseleave` only |
| Min-drag “fixed” to CSS px | Keep 2 **raster** px after mapping |

---

## 7. Definition of Done

Complete only when ALL hold:

- [ ] **DoD-1 — Frame coords:** Mouseup (and move) over an existing `.geom-overlay` persists a box in **image-frame** CSS space mapped through `css_point_to_raster`. `event_to_raster` / draw handlers do not use `offsetX`/`offsetY`. ui unit tests for `frame_css_point`.
- [ ] **DoD-2 — Draw cancel:** Changing document, page, or leaving the Image pane (or `mouseleave` on the frame) while `drawing=true` drops the in-flight drag; mouseup on the new page does not insert a geom from the old origin.
- [ ] **DoD-3 — Burn counts:** After `produce_burn_set` on a set that now has no remaining required burns, the Burn step shows Need burn **0** (patched from the burn response for **that** `item_ids`). Pre-burn QC findings / `ordered_ids` remain. Host test: response counts match `burn_counts_for_ids` after burn.
- [ ] **DoD-4 — Hygiene:** No `unwrap`/`expect` in new production code. No schema bump. 0114 raster/burn tests still pass. `cargo test -p dedupe-chrome` + ui `Cargo.toml` tests + trunk still green. 0119 latch tests still pass.
- [ ] **DoD-5 — Recorded:** `review.md`; registry **Completed**; CHANGELOG Unreleased sentence; `D-0120-pdf-raster-ui` closed; ledger committed (`BUGFIX`). **0121–0126** stay Proposed unless separately implemented.

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

Do **not** `git add` operator PSTs or `output/`.

---

## 9. Deferred absorb / decline

| ID | Disposition |
|---|---|
| **D-0120-pdf-raster-ui** | **Absorb — this track.** |
| **D-0114-pdfium-sidecar** | Remain (optional sidecar). |
| **D-0114-xform-text** | Remain (Form XObject). |
| **D-0034-05** | Remain (Acrobat-class viewer). |
| **D-0032-07** | Remain (inverse redact). |
| **D-0032-08** | Decline (operator GUI smoke). |
| **D-0121-image-opt-qc** | Remain (**0121**). |
| **D-0122-process-fold-residuals** | Remain (**0122**). |
| **D-0123-matter-shell** | Remain (**0123**). Owner locks recorded 2026-09-01: navy/blue tokens + Home under shared bar — not this track. |
| **D-0125-produce-canvas** | Remain (**0125**). Do not un-wizard here. |
| **D-0020-01** | Decline (operator GUI smoke). |
| **D-0062-codesign** | Remain. |
| Bugbot usage-limit on #127–#130 | **Decline** — not a product finding. |
| PR #119 three UI items | **Absorb** (this track; live-verified). |
| BCC-default | Never. |
| Fold-in 2026-09-01 (`opencode-review.md` + `agy-review.md`) | See table below. |

#### Harness fold-in (2026-09-01)

| Id | Disposition |
|---|---|
| opencode-m1 | **Agree — fold.** Min-drag guard is 2 **raster** px, not CSS. |
| opencode-m2 | **Agree — fold.** §2.5 pin 4172; drop stale impact-stale. |
| opencode-m3 | **Agree — fold.** Recount cloned burned-id list; `None`/empty skip. |
| opencode-O1 | **Agree — fold.** Pane-leave clear is a separate Effect on `pane`. |
| opencode-O2 | **Agree — fold.** Keep `mouseleave`, not `mouseout`. |
| opencode-O3 | **Already covered.** Conditional `git add -f`. |
| agy-M1 / M2 / M3 | **Already covered.** §3.1 / §3.2 / §3.3. |
| agy-m1 | **Agree — fold.** Clamp CSS points to the frame rect before mapping. |
| agy-m2 | **Already covered** (same as opencode-m1). |
| agy-O1 | **Already covered.** Keep overlay `pointer-events: auto`. |

---

## 10. Unblocks

Counsel can draw a box that overlaps an existing overlay and trust the burned native matches the visible drag. Burn-step counts stop lying after a set burn. **0125** can restyle the canvas on top of this honesty.
