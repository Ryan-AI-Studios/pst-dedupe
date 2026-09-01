# 0121 — Image OPT / QC residuals (PR #121 Bugbot)

> Structure follows `templates/0000-Description/spec.md`. DoD is §7.
> Phase 0 is **closed** in this file. Do not re-open unique-export, matter-home
> (**0110**), first-pass queue (**0111** / **0117**), review-window async
> (**0118**), DAT produce wizard honesty (**0113** / **0119**), zpdf burn
> compose (**0114**), Image-tab overlay/Burn counts (**0120**), Process
> extract-all (**0116** / **0122**), LFP/colour/email-print (**D-0115-***),
> or Series T canvas (**0123–0126**). Do not vendor `C:\dev\dedupe-frontend`.
> Do not mint a BCC-default track.

- **Track ID:** 0121-ImageOptQcResiduals
- **Execution repo:** `C:\dev\Dedupe`
- **Governance:** this directory in `C:\dev\Dedupe\conductor\` (track registry: `../conductor.md`)
- **Plan-of-record reference:** Hermes image produce + `qc_image_opt_v1`. `C:\dev\Dedupe-plan.md` is **absent** (re-verified 2026-09-01); do **not** chase it at execute.
- **Cross-repo contract:** mock at `C:\dev\dedupe-frontend` is research only (layout is **0125**; shell tokens are **0123**).
- **Status:** Completed
- **Depends on:** **0115 Completed** (PR **#121** / `19d0c1f`) · **0120 Completed** (do not retouch overlay/Burn counts) · schema **v41** (no bump)
- **Spec authored:** 2026-09-01 (placeholder → Ready)
- **Series:** O (Review chrome) — PR #121 image QC / eligibility residual
>
> **Closes / absorbs:** `D-0121-image-opt-qc` (this track). Does **not** close D-0122–D-0126, D-0115-lfp / D-0115-color / D-0115-email-print, D-0114-pdfium-sidecar, D-0114-xform-text, D-0062-codesign.
> **HITL:** owner launches the **release** chrome EXE on a synthetic image-profile matter: (1) cancel an in-flight image produce (`production_sets.status` `partial` / `running` / `failed` with `production_image_pages` and no `IMAGE.opt`) → QC / Finalize must **not** Error `opt_row_count_mismatch`; (2) complete volume, move/rename `output_root`, new Finalize of overlapping ids must **not** Error `image_page_missing` from the leftover; (3) a `.jpg` that is not JPEG magic ships native-only (no fail-closed); (4) a multi-IFD TIFF tagged `image/jpeg` produces one G4 page per IFD. INC* unique-pst is **not** a gate. Codesign is **D-0062-codesign**.
>
> **Last-PR fold-in (2026-09-01):** PRs **#133, #132, #131, #130, #129**. Disposition in §2.8. No new mint. Next free ID **0127**.
>
> **Harness fold-in (2026-09-01):** `opencode-review.md` + `agy-review.md`. Centerpiece: chrome (and explicit) `production_set_id` must **intersect** the QC selection; delete unused `looks_like_*_magic` helpers after QC delegates to `pdf-raster`. Status stays **Ready — not started**.
>
> **Stack lock (inherit 0110–0120):** Tauri **2** + Leptos **0.8 CSR** on **stable** Rust. Plex / paper / cool chrome. Red = privilege / withhold / blocker / draft redact overlay only. No daemon. No schema bump. `ui/` stays workspace-excluded. One pipeline. 0114 `redact_page` → rewrite unchanged. 0115 `wrap_g4_le_ifd` / `fax::tiff::wrap` forbidden on produce unchanged. 0119 `volume_succeeded` unchanged. Default DAT-only profile and `qc_default_v1` unchanged.

---

## 1. Objective

Keep **0115** image produce + `qc_image_opt_v1` **honest** on resume and mixed volumes: missing `IMAGE.opt` must not Error-block chrome Finalize while a volume is still in progress; QC must not fail a new Finalize because an old volume folder moved or a leftover complete set still overlaps the selection; JPEG/PNG path vs `sniff_kind` vs TIFF magic must agree so native-only items are not fail-closed after the fact, and multi-IFD TIFFs mis-tagged as JPEG still become G4 pages.

This is **correctness**. A QC Error that cannot pass on a resumable image job is the same honesty class as a silent unique-export drop. Unique-export itself is unchanged.

---

## 2. Context (read before starting)

### 2.1 Why this track, now

**0115 Completed** (PR **#121** / `19d0c1f`) shipped TIFF G4 + Opticon OPT and `qc_image_opt_v1`. Four **valid** Cursor Bugbot findings were parked here so **0116** could proceed. **0120 Completed** (PR **#131**) — do not retouch Image-tab coords or Burn-set recount. **0122** owns Process extract-all. **0125** owns un-wizard layout.

0115 already said: skip `opt_row_count_mismatch` on preflight when OPT is not written yet; Error when a **completed** image volume has pages and no OPT. Live code treats any persisted image pages + missing OPT as 0 lines then Error, which fires on resume.

### 2.2 Live APIs (plan-time 2026-09-01, HEAD `dff19e5`; review-time `f3d7a7c`; re-verify at execute)

Plan-time branch was `docs/series-t-and-how-to-build`. Review-time HEAD `f3d7a7c` is two docs commits later (PR **#133** Series T mint + 0121 Ready). `git diff dff19e5..f3d7a7c -- crates` was **empty** — product pins below still match. Re-verify line numbers on the execute branch.

| Surface | Fact |
|---|---|
| `crates/matter-core/src/schema.rs` | `SCHEMA_VERSION == 41`. **No schema bump this track.** |
| `crates/matter-qc/src/params.rs` `QcParams` | No `production_set_id`. Chrome `intended_qc_params` (`dedupe-chrome/src/produce.rs` ~575) is `item_ids` + `pack_id` only. |
| `evaluate_image_volume_rules` (`rules.rs` ~521–568) | `SELECT id, output_root, bates_prefix, profile_slug FROM production_sets WHERE matter_id = ?1` — **every** image set. No `status` column. |
| `evaluate_one_image_volume` OPT (~721–748) | Missing `IMAGE.opt` with `any_image_pages` → treat as **0 lines** → `opt_row_count_mismatch` Error. Skip only when there are no image pages. |
| `is_qc_image_eligible` (`rules.rs` ~473–498) | Path `.jpg`/`.jpeg`/`.png` (and MIME) returns true **before** magic. Duplicate of produce eligibility; `matter-qc` has **no** `pdf-raster` dep. |
| `pdf-raster/src/lib.rs` `sniff_kind` (~187–221) | PDF first, then JPEG magic **or JPEG MIME**, then PNG magic **or PNG MIME**, then TIFF magic / TIFF MIME / `.tif` path. MIME wins over later TIFF magic. |
| `is_image_eligible_native` (~226–234) | True if `sniff_kind` is not Other **or** path ends with `.jpg`/`.jpeg`/`.png`. Path-only JPEG/PNG with Other sniff is eligible. |
| `native_image_page_count` (`g4.rs` ~490–514) | Other → **0**. JPEG/PNG → 1. TIFF → IFD count. |
| `check_image_fail_closed` (`matter-produce/src/run.rs` ~2502–2521) | `page_count < 1` + `is_image_eligible_native` → fail the volume. Path-only JPEG with 0 pages fail-closes after native-only ship. |
| `write_image_opt` | Called at **end** of image finalize (`run.rs` ~2448), then fail-closed. Resume/preflight can have pages and no OPT. |
| Production set status | Insert `'running'`; cancel → `'partial'`; hard fail → `'failed'`; success → `'complete'` / `'complete_with_errors'`. |
| `list_production_sets_thin` | Latest `created_at` first. Fields: id, name, status, produced_ok_count, bates_prefix, next_seq, output_root. **No** `profile_slug`. |
| Test `run_production_qc_image_pack_missing_opt_on_completed_volume` (~978–1038) | One **complete** set, TIFFs on disk, **no** OPT → must fail. **Keep.** |
| MS-PST | **N/A this track.** |
| `zpdf` | **0.13.0** in `pdf-raster`. Do not bump. Do not change burn compose or G4 wrap. |

### 2.3 Mock + Hermes (research only)

Image profile: TIFF G4 + OPT. Produce checklist still `require_qc_pass`. Do not steal 0125 canvas or 0123 shell.

### 2.4 Plan-time crate pins (re-verify at execute)

| Crate | Plan-time | Rule |
|---|---|---|
| `tauri` | **2** | Reject **3.x / pre-release**. |
| `leptos` | **0.8.x** CSR | Do not bump major. |
| `zpdf` | **0.13.0** | Inherit; no compose change. |
| `fax` | **0.3.x** | G4 encode; `fax::tiff::wrap` still forbidden on produce. |
| Schema | **41** | No bump. |
| Rust | **stable** (CI) | No nightly. |
| trunk | **0.21.14** (ci.yml) | Keep. |

### 2.5 Tools / recall

Ran from `C:\dev\Dedupe`:

- `ai-brains preflight --summary` (inited; **4174** pinned at fold-in; plan-time recorded 4173 — cosmetic +1).
- Sync/recall: 0115 Ready/Completed (`ccbf7f15`, `8cc35214`) — opt-in G4 + OPT, schema v41, default DAT-only, `wrap_g4_le_ifd`. 0121 Ready pin `341e108d`.
- `ledgerful doctor --json` `readyForPublish: true`; warns phantom-promote, sig-pin, sig-version. `"completion-unreachable"` is not currently emitted. `"impact-stale"` may appear until the next scan — ignore.
- Ledger compact at fold-in: **0 pending / 0 unaudited drift**. Plan-time Docs tx `f4bc7a9f` landed in PR **#133** / `6a69256`. Execute starts `0121-image-opt-qc-residuals` **BUGFIX**.
- `ledgerful scan --impact` **LOW** on docs dirt at plan-time. Re-scan at execute after product edits. Federated scan hit the 5000-file budget under `output/` — ignore.

### 2.6 What we could not verify

Owner HITL on the release EXE (cancel-resume, moved volume folder, garbage `.jpg`, mis-tagged TIFF). Execute re-reads `sniff_kind` order and OPT skip vs `status` on live HEAD.

### 2.7 Related deferred (roll)

See §9. Absorb **D-0121-image-opt-qc**. Remain D-0115-lfp / color / email-print. Decline D-0032-08 / D-0020-01 as operator smoke.

### 2.8 Last-PR Cursor comments (2026-09-01)

PRs **#133, #132, #131, #130, #129**. Inline review comments empty. Issue comments are Bugbot **usage-limit** only (no findings). **Decline** as product input. **#133** landed after the first Ready pass (Series T mint / `6a69256`); same usage-limit notice, no product finding.

Origin PR **#121** still has the four items this track owns (live-verified at §2.2; PR-branch line numbers drifted from `99b054b`).

| Origin | Verdict |
|---|---|
| #121 High — QC OPT blocks resume | **Absorb** |
| #121 High — QC walks every `production_sets` row | **Absorb** |
| #121 Medium — JPEG path vs sniff vs fail-closed | **Absorb** |
| #121 Medium — MIME wins over TIFF magic | **Absorb** |
| #133–#129 usage-limit | **Decline** |

No new mint. Next free ID **0127**.

### 2.9 Product locks (do not invent at execute)

**OPT (High 1).** `opt_row_count_mismatch` Errors on missing/mismatched OPT **only** when the evaluated set `status` is `complete` or `complete_with_errors`. For `running` / `partial` / `failed` (and any other non-complete status) with persisted pages and no OPT: **skip** the OPT rule (do not Warn-as-Error). Keep the completed-volume missing-OPT test. Produce-internal `check_image_fail_closed` after `write_image_opt` stays the post-produce gate.

**Set scope (High 2).** Do not evaluate every image volume on every QC run.

1. Add optional `production_set_id: Option<String>` on `QcParams` (`#[serde(default, skip_serializing_if = "Option::is_none")]`). JSON additive. **No schema bump.** Absent field must deserialize as None so existing fingerprints stay stable.
2. Pass it from `run.rs` into `evaluate_image_volume_rules`. `evaluate_candidates*` may pass `None`.
3. Selection:
   - If `production_set_id` is Some and that row is an image set (`production_set_has_images`) **and** it intersects `candidate_ids` (any `production_items.item_id` on that set is in the selection; Bates/`SKIP_*` not required): evaluate **that set only**. If the id is unknown, not an image set, or **does not intersect** the selection: **ignore the id** and use the unset heuristic (do not skip image QC entirely; do not scope to a stale `partial` / unrelated running volume).
   - If unset: evaluate non-complete image sets that intersect `candidate_ids` (resume). If none: evaluate complete image sets **only when there is exactly one** image set in the matter (legacy single-volume audit + existing unit test). If two or more complete image sets and no in-progress set: **skip** post-volume disk/OPT/span/multi-tiff rules (new Finalize must not inherit leftover volumes).
4. Always skip a set whose `output_root` is missing/unreadable for **disk** TIFF checks. A complete leftover with a moved folder must not emit `image_page_missing`.
5. Chrome: `intended_qc_params` / `produce_qc_run_blocking` pass a set id only when that set **intersects `ordered`**. Prefer a non-complete intersecting thin set, else the latest complete intersecting thin set (`list_production_sets_thin` is newest first) whose `output_root` exists on disk, else omit (`None`) and let the unset heuristic decide. Intersection: any `production_items.item_id` for the set is in `ordered` (host may query; `list_ok_production_controls` is acceptable if it still intersects resume rows). This is host wiring, **not** 0125 canvas and **not** 0119 latch. DAT-only `qc_default_v1` runs still omit image rules; do not change pack mapping.

**JPEG eligibility (Medium 3).** `is_image_eligible_native` is true only when `sniff_kind` is not Other. **Remove** the path-only `.jpg`/`.jpeg`/`.png` fallback. Path-only without magic or MIME → native-only, not fail-closed. Align `is_qc_image_eligible` with that function after the native-only kind check (add `pdf-raster` dep to `matter-qc`; do not keep a path-first duplicate). Keep native-only EML/xlsx/csv/pptx as today.

**TIFF vs MIME (Medium 4).** `sniff_kind` order after PDF (`detect_pdf` / `looks_like_pdf` stays first): **TIFF magic** (`II*`/`MM*`), **JPEG magic**, **PNG magic**, then **path** (`.tif`/`.tiff`, `.jpg`/`.jpeg`, `.png`), then **MIME**. TIFF magic before JPEG/PNG MIME. A multi-IFD TIFF tagged `image/jpeg` is `NativeKind::Tiff` and `native_image_page_count` is the IFD count. After magic, **path beats MIME** (a `.png` named file with MIME `image/tiff` sniffs Png). That case is not DoD-5; do not invent a MIME-over-path exception.

Do not change 0114 burn compose, 0115 G4 wrap, Highlights-never-burn, privilege-in-set, `fail_if_withheld`, `require_qc_pass`, 0119 `volume_succeeded`, 0120 overlay/Burn recount, or default DAT-only / `qc_default_v1`.

---

## 3. In scope

Image QC set selection + OPT skip-until-complete; `sniff_kind` / eligibility alignment; chrome QC params set id when known; tests listed in §7.

### 3.1 OPT skip until complete

`evaluate_one_image_volume` takes set `status`. OPT Error only for `complete` / `complete_with_errors`. In-progress with pages and no OPT returns existing non-OPT findings only.

### 3.2 Scope image QC to the current job

`evaluate_image_volume_rules` selects sets per §2.9. SELECT must include `status`. Chrome passes `production_set_id` only for a thin set that **intersects** `ordered`.

### 3.3 Eligibility matches page prediction

One `sniff_kind`. `is_image_eligible_native` has no path-only JPEG/PNG override. QC helper calls it after the native-only kind check. **Delete** the now-unused `looks_like_{pdf,jpeg,png,tiff}_magic` helpers in `rules.rs` (clippy `-D warnings` / `dead_code` otherwise). `check_image_fail_closed` then agrees with `native_image_page_count == 0`.

### 3.4 Magic before MIME

Reorder `sniff_kind` as §2.9. Tests in `crates/pdf-raster/tests/g4.rs`.

---

## 4. Out of scope (do NOT do here)

- Process extract-all Busy wipe / orphan jobs (**0122**).
- Image-tab overlay mouseup / draw cancel / Burn-set recount (**0120** Completed).
- Finalize latch / privilege-log empty-set (**0119** Completed).
- Un-wizard produce canvas (**0125**).
- Matter shell tokens / Home under bar (**0123**).
- IPRO LFP, colour JPEG pages, EML/OOXML print-to-TIFF (**D-0115-***).
- Schema bump, BCC-default, `qc_default_v1` severity table, default DAT-only profile.
- In-tool ScanPST / CRC repair. Source PST mutation. Client evidence in git.

---

## 5. Preconditions & dependencies

- **P1 (blocking):** **0115 Completed**. Schema **41**.
- *Verified to date:* four Bugbot sites still present on HEAD `dff19e5` (§2.2). Completed-volume missing-OPT test still requires fail.

---

## 6. Risks

| Risk | Mitigation |
|---|---|
| Skipping all complete sets hides a real missing OPT on the volume just produced | Chrome passes latest **intersecting** complete set id with existing root; produce-internal fail-closed still runs at job end; single-complete heuristic covers CLI/tests without set id. |
| Chrome picks a stale `partial` / unrelated running set and scopes QC off the selection's volume | §2.9.3 + §2.9.5: set id only if it intersects `ordered` / `candidate_ids`; else omit and use the unset heuristic. |
| `production_set_id` in QcParams changes fingerprints | Skip serializing when None; absent key == None. |
| `pdf-raster` dep pulls zpdf into matter-qc | Acceptable; one eligibility function. Do not reimplement a second sniff order. |
| Disk checks on in-progress volumes | Skip disk when root missing; keep DB page-count vs `production_image_pages` for in-scope items. |

---

## 7. Definition of Done

Complete only when ALL hold:

- [x] **DoD-1 — Resume OPT:** QC on a **non-complete** image set (`running` / `partial` / `failed`) with persisted `production_image_pages` and no `IMAGE.opt` does **not** emit Error `opt_row_count_mismatch`. Chrome Finalize can pass `require_qc_pass`. Integration test next to `run_production_qc_image_pack_missing_opt_on_completed_volume`.
- [x] **DoD-2 — Completed OPT:** The existing complete-volume missing-OPT test still fails QC. Explicit `production_set_id` of that complete set (if used) still Errors.
- [x] **DoD-3 — Leftover / moved volume:** Two complete image sets; one `output_root` missing; overlapping `candidate_ids`; QC **without** that leftover as the only target does **not** Error `image_page_missing` / OPT from the missing folder. New Finalize of overlapping ids is not blocked by the leftover.
- [x] **DoD-4 — JPEG path:** Path `.jpg`/`.jpeg`/`.png` with Other sniff is **not** image-eligible. `native_image_page_count` is 0. Produce does not fail-closed. QC does not Error `image_page_missing` for that item solely from the extension.
- [x] **DoD-5 — TIFF magic:** Bytes with TIFF `II*`/`MM*` and MIME `image/jpeg` (or `image/png`) sniff as `Tiff`. `native_image_page_count` equals IFD count (fixture with ≥2 IFDs).
- [x] **DoD-6 — Hygiene:** No `unwrap`/`expect` in new production code. No schema bump. `qc_default_v1` and default DAT-only unchanged. Unused `looks_like_*_magic` helpers gone. `cargo test -p matter-qc` + `-p pdf-raster` + `-p matter-produce` + chrome host tests that construct `QcParams` / `intended_qc_params` (picker omits a non-intersecting `partial`). 0119 latch and 0120 overlay tests still pass.
- [x] **DoD-7 — Recorded:** `review.md`; registry **Completed**; CHANGELOG Unreleased sentence; `D-0121-image-opt-qc` closed; ledger committed (`BUGFIX`). **0122–0126** stay Proposed unless separately implemented.

---

## 8. Verification commands (reference)

```powershell
Set-Location C:\dev\Dedupe
cargo test -p matter-qc
cargo test -p pdf-raster
cargo test -p matter-produce
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
| **D-0121-image-opt-qc** | **Absorb — this track.** |
| **D-0115-lfp** | Remain (IPRO LFP). |
| **D-0115-color** | Remain (colour JPEG pages). |
| **D-0115-email-print** | Remain (EML/OOXML print-to-TIFF). |
| **D-0114-pdfium-sidecar** | Remain. |
| **D-0114-xform-text** | Remain. |
| **D-0120-pdf-raster-ui** | Closed in **0120**. Do not reopen. |
| **D-0122-process-fold-residuals** | Remain (**0122**). |
| **D-0123-matter-shell** | Remain (**0123**). |
| **D-0125-produce-canvas** | Remain (**0125**). Do not un-wizard here. |
| **D-0032-08** | Decline (operator GUI smoke). |
| **D-0020-01** | Decline (operator GUI smoke). |
| **D-0062-codesign** | Remain. |
| Bugbot usage-limit on #129–#133 | **Decline** — not a product finding. |
| PR #121 four QC/eligibility items | **Absorb** (this track; live-verified). |
| BCC-default | Never. |
| Fold-in 2026-09-01 (`opencode-review.md` + `agy-review.md`) | See table below. |

#### Harness fold-in (2026-09-01)

| Id | Disposition |
|---|---|
| opencode-M1 | **Agree — fold.** Chrome + explicit `production_set_id` must intersect the QC selection; else ignore id / omit and use the unset heuristic. |
| opencode-m1 | **Agree — fold.** Delete unused `looks_like_*_magic` helpers after QC delegates to `pdf-raster`. |
| opencode-m2 | **Agree — fold.** Pins: preflight **4174**; last-PR window includes **#133**; product crates unchanged `dff19e5..f3d7a7c`. |
| opencode-O1 | **Decline.** Silent skip of ≥2 complete sets is required by DoD-3. A new Warn “skipped N volumes” finding expands `qc_image_opt_v1` and is not this DoD. |
| opencode-O2 | **Already covered.** §8: do not `git add` stray `agy-review.md` / `fixtures/keep_set_summary.json`. |
| opencode-O3 | **Agree — fold.** Path-after-magic beats MIME; not a DoD-5 exception. |
| agy-M1 / M2 / M3 | **Already covered.** §2.9 OPT-until-complete / set scope / TIFF magic-first. |
| agy-m1 / m2 | **Already covered.** Path-only JPEG fallback removal + `pdf-raster` delegate. |
| agy-O1 | **Already covered.** Single-complete unset fallback. |

---

## 10. Unblocks

Counsel can resume an interrupted image produce and Finalize without a lying OPT Error. A moved leftover volume cannot poison the next image job. JPEG/PNG path and TIFF magic agree with page counts so fail-closed does not fire on native-only items or drop interior TIFF IFDs.
