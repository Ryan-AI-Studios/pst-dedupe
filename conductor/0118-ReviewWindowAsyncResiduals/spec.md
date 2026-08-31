# 0118 — Review-window async residuals (PR #115 Bugbot)

> Structure follows `templates/0000-Description/spec.md`. DoD is §7.
> Phase 0 is **closed** in this file. Do not re-open unique-export, matter-home
> (**0110**), first-pass queue (**0111** / **0117**), produce (**0113** / **0119**),
> zpdf burn compose (**0114**), Image-tab mouseup/draw-state/Burn counts (**0120**),
> TIFF/OPT (**0115** / **0121**), or Process (**0116** / **0122**). Do not vendor
> `C:\dev\dedupe-frontend`. Do not mint a BCC-default track.

- **Track ID:** 0118-ReviewWindowAsyncResiduals
- **Execution repo:** `C:\dev\Dedupe`
- **Governance:** this directory in `C:\dev\Dedupe\conductor\` (track registry: `../conductor.md`)
- **Plan-of-record reference:** Hermes review window. `C:\dev\Dedupe-plan.md` is **absent** (re-verified 2026-08-31); do **not** chase it at execute.
- **Cross-repo contract:** mock at `C:\dev\dedupe-frontend` is research only (no `/review/:docId`).
- **Status:** In Progress
- **Depends on:** **0112 Completed** (PR **#115** / `81a3aad`) · schema **v41** (no bump)
- **Spec authored:** 2026-08-31 (placeholder → Ready)
- **Series:** O (Review chrome) — PR #115 window async residual
>
> **Closes / absorbs:** `D-0118-review-window-async` (this track). Does **not** close D-0119–D-0122, D-0117 (already closed), D-0110-deny-unic, D-0020-01, D-0062-codesign.
> **HITL:** owner launches the **release** chrome EXE, opens a **synthetic** 3-doc Unreviewed family, rapid Save & Next / `[` `]`, then **Enter on the last item** (no Next), un-check privilege, Save/Enter. INC* unique-pst is **not** a gate. Codesign is **D-0062-codesign**.
>
> **Last-PR fold-in (2026-08-31):** PRs **#126, #125, #124, #123**. Disposition in §2.8. No new mint. Next free ID **0123**.
>
> **Review fold-in (2026-08-31):** `opencode-review.md` + `agy-review.md`. Disposition in §2.10 and `foldin-note.md`.
>
> **Stack lock (inherit 0110–0117):** Tauri **2** + Leptos **0.8 CSR** on **stable** Rust. Plex / paper / cool chrome. Red = privilege / withhold / blocker only. No daemon. No schema bump. `ui/` stays workspace-excluded. One pipeline.

---

## 1. Objective

Keep the **0112** three-pane window **honest under Save & Next / `[` `]`**: a slower `review_document` or `review_document_body` reply must not paint item B’s headers or `<pre class="doc-body">` on item C, and a successful persist that **stays on the same item** must refresh `doc.codes` / `doc.notes` so the next save diffs current membership (privilege/confidential off actually emits a remove; new notes appear in History).

This is **correctness**, not chrome polish. Coding the wrong document, or a no-op un-privilege because the client still thinks the code is off, is a silent drop of counsel intent — the same honesty class as unique-export. Unique-export itself is unchanged.

---

## 2. Context (read before starting)

### 2.1 Why this track, now

**0112 Completed** (PR **#115** / `81a3aad`) shipped the money screen. Three **valid** Cursor Bugbot findings were parked here so **0113** could proceed. **0117 Completed** (PR **#125** / `199975c`). This `/plan-track 118` expands the placeholder.

### 2.2 Live APIs (plan-time 2026-08-31, HEAD `53c9f05`; re-verify at execute)

| Surface | Fact |
|---|---|
| `crates/matter-core/src/schema.rs` | `SCHEMA_VERSION == 41`. **No schema bump this track.** |
| `ui/src/pages/review_window.rs` | Document Effect (~290–355) `spawn_local` `review_document` then `doc.set` with **no** gen/`item_id` check. Body Effect (~357–383) `spawn_local` `review_document_body` then `body.set` with **no** check. Raster Effect (~395–459) **already** uses `raster_generation` + `r.item_id == doc_id` — **copy that pattern**; do not rewrite Image-tab draw (**0120**). |
| Persist (~472–671) | `codes_state(&d.codes)` diffs add/remove. **All four call sites pass `then_next=true`** (Enter, Ditto, Confirm, Save & Next). End of queue is `then_next && next_id is None` → `"End of queue"` (~666) — **not** the `then_next=false` `"Saved."` arm (~668–670), which is currently **dead**. No `doc` refresh after stay. |
| `ReviewDocument` | `item_id`, `codes`, `notes`, `privilege`, `prev_id`/`next_id`, family card. Host `review_document_blocking` (`document.rs`) already returns them. |
| `ReviewDocumentBody` | Echoes `item_id` + `pane`. Host `body.rs` already fills both. |
| Host generation | `review_raster_page` / `review_geom_list` take `generation: Option<u64>` (0114). **`review_document` / `review_document_body` do not** — this track stays **UI-side** (do not add host fields unless a finding proves it). |
| `ui/src/path_id.rs` | `review_doc_href_encodes_filter_and_keyword` has **two** `#[test]` (~111–112). `stub_back_href_reencodes_decoded_windows_param` (~127) has **none**. |
| `review_window_apply` | 0112 lock: pre-check basis → `apply_codes` → upsert; compensate on upsert fail. **Do not change** this sequence or `ensure_item_privilege_conn`. |
| Body display | CAS prefix + `from_utf8_lossy` + copied html strip. **Never** `innerHTML`. Unchanged. |
| CI | `chrome-ui`: trunk build + `cargo test -p dedupe-chrome`. **Does not** run the ui crate’s `#[test]`s today. |
| MS-PST | **N/A this track.** |

### 2.3 Mock + Hermes (research only)

No `/review/:docId` in the mock. Inherit 0112: Resp ⊥ Privilege; ditto is `d`; `3` is Needs review, not Privileged.

### 2.4 Plan-time crate pins (re-verify at execute)

| Crate | Plan-time | Rule |
|---|---|---|
| `tauri` | **2** | Reject **3.x / pre-release**. |
| `leptos` | **0.8.x** CSR | Do not bump major. |
| Schema | **41** | No bump. |
| Rust | **stable** (CI) | No nightly. |

### 2.5 Tools / recall

Ran from `C:\dev\Dedupe`:

- `ai-brains preflight --summary` (inited; 4129 pinned).
- Recall: 0112 Ready/Completed locks `review_window_apply` sequence, family_members_thin LIMIT, catalog read-first; 0113 minted this ID (not stolen into produce).
- `ledgerful doctor --json` `readyForPublish: true`; `ledger status --compact` **0 pending / 0 unaudited drift** before this tx. Doctor: phantom-promote, impact-stale, sig-pin, sig-version, completion-unreachable — none block planning.
- Ledger tx for this planning pass: `eaad4c78-cf7e-4400-807e-3adf2ee4de41`.
- `scan --impact` LOW (docs/conductor expected). Stay inside `review_window.rs` + `path_id.rs` (+ optional chrome-ui test step).

### 2.6 How this advances the north star

Save & Next is the 400-docs/hour loop. Painting the wrong body, or a second save that cannot remove privilege because `doc.codes` is stale, is a **silent drop of coding intent**. Not UI chrome for its own sake.

### 2.8 Last-PR Cursor comments (mandatory)

Last four merged product PRs: **#126** (docs 0117), **#125** (0117 queue), **#124** (docs 0116), **#123** (0116 Process fold).

| PR | Surface | Disposition |
|---|---|---|
| **#126** | docs registry | Bugbot usage-limit comment only; **no** inline findings |
| **#125** | Queue virtualization | Bugbot usage-limit comment only; **no** inline findings |
| **#124** | docs registry | no inline comments (prior session: usage-limit) |
| **#123** | Process + Produce | Already **0119** (cancelled-produce) + **0122** (extract-all / orphan). Do not steal into 0118. |

PR **#115** (this track’s origin; not in the last-four window) — three window comments still live at HEAD `53c9f05`:

| Bugbot id | Severity | Fold |
|---|---|---|
| `d4f586ec` Stale review loads | High | **This track** §3.1 |
| `e7aae96b` Saved snapshot | Medium | **This track** §3.2 |
| `ef6ecfe4` Unused path_id `#[test]` | Low | **This track** §3.3 |

No BCC-default track. No new placeholder. Next free ID **0123**.

### 2.9 Product locks (do not invent at execute)

See §3. Inherit 0112: Resp ⊥ Privilege; family propagate default off; `review_window_apply` sequence; never `innerHTML`; Image-tab draw residuals stay **0120**.

### 2.10 Review fold-in (2026-08-31)

Sources: `opencode-review.md`, `agy-review.md`. Inputs not edited.

| Id | Sev | Disposition | Lock |
|---|---|---|---|
| opencode-M1 + agy-M2 | Major | **Agree — fold** | Same-item refresh when persist **does not navigate**: `then_next && next_id is None` (live end-of-queue). Also run it if a future `then_next == false` caller appears. **No** new Save-without-Next button this track. |
| opencode-M2 | Major | **Agree — fold** | Catalog invoke in the document Effect gated by the same `fetch_is_current` (id+gen). |
| agy-M1 | Major | **Already covered** | §3.1 already guards Ok **and** Err for document/body. Catalog is opencode-M2. |
| agy-M3 | Major | **Agree — fold** | Post-save re-fetch **increments** `doc_generation` before spawn (same as §3.1). |
| opencode-m1 | Minor | **Agree — fold** | When the refresh is current, overwrite `pending_*` from the new doc (unconditional). |
| opencode-m2 | Minor | **Agree — fold** | Same-item refresh **failure** → `status` only; do **not** `error.set` or `doc.set(None)`. |
| agy-m1 | Minor | **Already covered** | Body already requires `b.pane == pane`. |
| agy-m2 | Minor | **Already covered** | §3.3 path_id `#[test]`. |
| agy-O1 | Opportunity | **Already covered** | chrome-ui ui `Cargo.toml` tests. |
| opencode-m3 / m4 / m5 | Minor | **Decline** | Pin-count / doctor-line / `git add -f` note cosmetic. |

---

## 3. In scope

UI + `path_id` tests (+ a small chrome-ui test step). **Do not** change `review_window_apply` host sequence, FilterSpec, queue virtualization, Process, Produce, or Image-tab geom math.

### 3.1 Stale document/body must not apply (`d4f586ec`)

- Copy the **live raster** pattern: increment a counter **before** `spawn_local`; after await, apply **only if** current.
- Pure helper (ui crate, unit-tested):

  `fn fetch_is_current(want_id: &str, want_gen: u64, got_id: &str, got_gen: u64) -> bool`

  True iff `want_id == got_id && want_gen == got_gen`.
- **Document Effect:** `doc_generation` increment; capture `id` at spawn; on OK/Err apply `doc` / `error` / `loading` / **`catalog`** only when `fetch_is_current(id, gen, doc_id.get_untracked(), doc_generation.get_untracked())`. Pending radios/checkboxes update only on that same OK. Catalog `set` / catalog error: same gate (stale matter’s code ids must not land).
- **Body Effect:** `body_generation` increment; capture `id` + `pane`; on OK apply `body` only when current **and** `b.pane == pane` (stale native must not replace text after a pane switch). Errors: same guard (do not paint Body: … on the new item).
- Do **not** require host `generation` fields on `review_document` / `review_document_body`.
- Showing the previous item under `loading=true` until the **current** reply arrives is allowed. Clearing to blank-flash is **not** required.
- Do **not** touch `raster_generation` / Image overlay / draw state (**0120**).

### 3.2 Same-item persist refreshes codes/notes (`e7aae96b`)

- Trigger: after a **successful** persist that **does not navigate** — live path is `then_next == true` **and** `next_id` was `None` (`"End of queue"`). Also honor `then_next == false` if a caller is added later. **Do not** mint a Save-without-Next button this track (all four live sites pass `true`).
- Re-invoke `review_document` for the same `item_id` + current filter/keyword. **Increment `doc_generation` before spawn** (agy-M3). Apply the OK result only via `fetch_is_current` (a later `[` must win).
- Refresh `doc.codes`, `doc.notes`, `doc.privilege` (and neighbors if the payload includes them). When that OK is current, overwrite `pending_*` from `codes_state` of the **new** doc (unconditional — the refetch is the post-save snapshot).
- Refresh **failure**: set `status` (e.g. `"Saved, but refresh failed: …"`). Do **not** `error.set`, do **not** `doc.set(None)` (the save already succeeded).
- When persist **does** navigate (`then_next && Some(next_id)`): `go_item` only; do **not** patch the old item.
- Do **not** invent a new host apply-result DTO. Re-fetch is the lock (notes History + privilege claim must match SQLite).
- Do **not** change add/remove diff rules except that they read the refreshed `doc`.

### 3.3 Path encoding test runs (`ef6ecfe4`)

- One `#[test]` on `review_doc_href_encodes_filter_and_keyword` (drop the duplicate attr).
- Put `#[test]` on `stub_back_href_reencodes_decoded_windows_param` (Windows decoded-param re-encode). Do not rename away the regression.
- **CI:** add `cargo test --manifest-path crates/dedupe-chrome/ui/Cargo.toml` to the `chrome-ui` job (host target; these tests are not wasm-only). Today trunk-build + `cargo test -p dedupe-chrome` never runs `path_id.rs` tests.

### 3.4 Tests (normative)

1. `fetch_is_current`: match → true; id mismatch → false; gen mismatch → false.
2. `stub_back_href_reencodes_decoded_windows_param` runs and asserts `%3A` / `%5C`.
3. `review_doc_href_encodes_filter_and_keyword` still passes (single `#[test]`).
4. Existing host `cargo test -p dedupe-chrome` still passes (document/body host tests unchanged).

HITL (DoD-5): rapid `[` `]` / Save & Next never shows the previous subject on the new headers with the old `<pre class="doc-body">`; **Enter on the last item** (end of queue, no Next) then un-check privilege + Save/Enter emits a remove (History / PRIV pill).

---

## 4. Out of scope (do NOT do here)

- Queue header/spacer/arrows (**0117**, closed).
- Produce Finalize / cancelled-as-success (**0113** / **0119**).
- Image-tab mouseup coords / drag-across-page / Burn counts (**0120**).
- Image OPT QC (**0121**). Process extract-all / orphan (**0122**).
- Changing `review_window_apply` sequence or `ensure_item_privilege_conn`.
- Host `generation` on `review_document` / `review_document_body`.
- `innerHTML`, family propagate default, Resp⊥Privilege keyboard table.
- Schema v42. BCC-default. Gutting `dedupe-desk`.

---

## 5. Preconditions & dependencies

- **P1 (blocking):** 0112 Completed; `review_window.rs` + `path_id.rs` still as §2.2.
- *Verified to date:* three Bugbot sites still live on HEAD `53c9f05`; raster already guarded; schema v41; chrome-ui does not run ui crate tests.

---

## 6. Risks

| Risk | Mitigation |
|---|---|
| Stale body after pane switch | Guard on `item_id` **and** `pane` |
| Stale catalog / error on `[` `]` | Gate catalog + Err with `fetch_is_current` |
| Same-item refetch races `[` | Increment `doc_generation` before post-save spawn |
| Refresh fail blanks a saved doc | `status` only; keep `doc` |
| `then_next=false` never runs | Refresh on **did not navigate** (`next_id` None) |
| Touching Image-tab draw | Fence **0120**; do not edit overlay mouse handlers |
| Host apply sequence drift | Do not edit `review_window_apply` |
| Restored test still unused in CI | chrome-ui `cargo test --manifest-path …/ui/Cargo.toml` |

---

## 7. Definition of Done

Complete only when ALL hold:

- [ ] **DoD-1 — Stale fetch:** Rapid Save & Next / `[` `]` never applies a slower previous `review_document` / `review_document_body` to the current item (generation + id; body also pane). `fetch_is_current` tests pass.
- [ ] **DoD-2 — Same-item save:** Persist that does **not** navigate (`then_next && next_id is None`, or `then_next == false`) increments `doc_generation`, re-fetches `review_document`, refreshes codes/notes/privilege. Refresh fail keeps `doc`. A follow-up save can remove privilege/confidential. Notes appear in History.
- [ ] **DoD-3 — path_id:** Windows-root encoding test runs (`#[test]` on the original fn); duplicate `#[test]` removed. chrome-ui job runs ui crate tests.
- [ ] **DoD-4 — Hygiene:** No `unwrap`/`expect` in new production chrome/ui. No schema bump. `review_window_apply` sequence unchanged. Raster/overlay/draw not rewritten. `cargo test -p dedupe-chrome` + ui `Cargo.toml` tests + trunk still green.
- [ ] **DoD-5 — Recorded:** `review.md`; registry **Completed**; CHANGELOG Unreleased sentence; `D-0118-review-window-async` closed; ledger committed (`BUGFIX` or `FEATURE`). **0119–0122** stay Proposed unless separately implemented.

---

## 8. Verification commands (reference)

```powershell
Set-Location C:\dev\Dedupe
cargo test --manifest-path crates/dedupe-chrome/ui/Cargo.toml
cargo test -p dedupe-chrome
# chrome-ui trunk (re-verify workflow file at execute)
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
| **D-0118-review-window-async** | **Absorb — this track.** |
| **D-0117-queue-virtualization** | Remain closed (**0117** / PR **#125**). |
| **D-0119-produce-checklist-residuals** | Remain (**0119**). |
| **D-0120-pdf-raster-ui** | Remain (**0120**). Do not steal overlay/draw. |
| **D-0121-image-opt-qc** | Remain (**0121**). |
| **D-0122-process-fold-residuals** | Remain (**0122**). |
| **D-0020-01** | Decline (operator GUI smoke). |
| **D-0110-deny-unic** | Remain (upstream unic). |
| **D-0062-codesign** | Remain. |
| Host `generation` on document/body | **Decline** — UI-side counters suffice. |
| opencode-M1 `then_next=false` dead branch | **Absorb** — folded into §3.2 (did-not-navigate). |
| New Save-without-Next button | **Decline** — not this track. |
| BCC-default | Never. |

---

## 10. Unblocks

Counsel can Save & Next without coding the wrong document, and can un-privilege at end of queue. **0119–0122** stay independent.
