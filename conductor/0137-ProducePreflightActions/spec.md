# 0137 — Produce pre-flight actions (real blockers)

> Keep **0125** un-wizard (five steps visible + Stage) and **0119** Finalize latch.
> Do not port ACME VOL002 theater or fake 8,441 docs. **This is not a greenfield link pass.**

- **Track ID:** 0137-ProducePreflightActions
- **Execution repo:** `C:\dev\Dedupe`
- **Governance:** this directory in `C:\dev\Dedupe\conductor\`
- **Status:** Ready — not started
- **Depends on:** **0125 Completed** (PR **#143**) · **0119 Completed**
- **Spec authored:** 2026-09-03 (placeholder) → **2026-09-03 Ready** (HEAD `cc88576`)
- **Series:** V

> **Closes / absorbs:** `D-0137-produce-preflight-actions`. Does **not** rewrite 0125 canvas or 0119 latch.
> **HITL:** produce pre-flight: extras **with** `item_id` still open review; extras **without** (`qc_gate`, `empty_selection`, `privilege_log_blank`) jump to the real pane (QC / Set / protocol). Finalize still blocked until extras clear.

---

## 1. Objective

Where the mockup is superior — **blocker/warn rows with a next action** — finish chrome pre-flight cards so every extra has either a working jump or **no** dead button. Keep chrome’s real QC gate copy (Missing / run production QC) instead of replacing it with mock “4 blockers.”

---

## 2. Context (read before starting)

### 2.1 Live APIs (`cc88576`; **re-verify at execute**)

| Surface | Fact |
|---|---|
| `ChromeExtra` | `kind, severity, item_id: Option<String>, message`. |
| UI `produce.rs` ~1170 | Extras **already** render `<A href=h>"Open in review"</A>` when `item_id` is Some (`review_doc_href`). Findings do the same. Warn findings already have **Record override**. Header already has **Re-run QC**. |
| Extras **without** `item_id` (plan-time) | `empty_selection`, `qc_gate`, `privilege_log_blank` (`chrome_extras` / `privilege_log_blank_blocker`). |
| Extras **with** `item_id` | `uncoded_in_set` (per candidate id); QC error findings copied as extras at Finalize; `pdf_raster_failed` warnings. |
| Protocol pane | Privilege protocol is a sibling card (`class="produce-protocol"`) — **no** `id` for hash jump at plan-time. Step nav already has `#step-1-set` … `#step-5-preflight`. |
| Review | `/matters/:id/review` and `/matters/:id/review/:docId`. Queue `extras` bool is **confidential columns**, not a QC findings queue. Do **not** invent an “Open QC queue” page. |
| Latch | **0119** Finalize `volume_succeeded` / extras blockers — **frozen**. |
| Layout | **0125** five steps + Stage — **frozen**. |
| Schema | **41**. No bump. |
| MS-PST | **N/A this track.** |

### 2.2 Remaining mockup-superior gap

Not “add Open in review.” That shipped. Remaining:

| Extra `kind` | Action |
|---|---|
| `item_id` Some | Keep **Open in review** (`review_doc_href`) via Leptos `<A>`. |
| `qc_gate` | **Re-run QC only** (button already on the pre-flight card). Do **not** add `#step-5-preflight` self-hash (cards already live inside that step). Do not mint a QC-queue route. |
| `empty_selection` | In-page **plain** `<a href="#step-1-set">` (same element type as the step nav — not `<A>`). |
| `privilege_log_blank` | Plain `<a href="#privilege-protocol">` after adding `id="privilege-protocol"` on the protocol pane. `include_str` lock on that id + href. |
| Findings without `item_id` | Text only (fail-closed). |
| Unknown kind + no `item_id` | No action, no dead button. |

Button chrome may match mockup labels **only** when the target exists.

### 2.3 Locks

No fake snapshot; no Bates ACME; privilege-in-set unchanged; Stage pane stays. Unique-export unchanged.

### 2.4 Tools / comments

Same as 0133. Decline Bugbot usage-limit.

---

## 3. In scope

1. Kind-dispatch helper (unit-tested, including `unknown_kind` + no `item_id` → no href).
2. `id="privilege-protocol"` on the protocol pane; **plain `<a>`** for in-page hashes; `<A>` only for `review_doc_href`.
3. Tests: extra with `item_id` still builds `/matters/:id/review/:docId`; `qc_gate` has no review href and no self-hash; `empty_selection` → `#step-1-set`; `privilege_log_blank` → `#privilege-protocol`; `include_str` `id="privilege-protocol"`; latch tests unchanged.

## 4. Out of scope

- New produce math or latch rewrite.
- New Review QC-queue page.
- Process unaccounted (**0133**).
- Admin tab (0123 inert).
- D-0125-dead-css (cosmetic leftover; remain unless one-line while touching `app.css`).

## 5. Preconditions

- **P1:** 0125 un-wizard + 0119 latch in live chrome.
- *Verified:* Open in review already exists for extras/findings with ids.

## 6. Risks

| Risk | Mitigation |
|---|---|
| Rewriting Finalize | Forbidden |
| Dead “Open QC queue” | Use Re-run QC + `#step-5-preflight` only |

## 7. Definition of Done

- [ ] **DoD-1** 0125 layout + 0119 latch unchanged (existing latch tests pass).
- [ ] **DoD-2** Navigable extras: `<A>` + `review_doc_href` when `item_id` is Some; plain `<a>` hashes for `empty_selection` / `privilege_log_blank`; `qc_gate` is Re-run QC only. Unknown kind with no id has no button. Kind-dispatch tests pass.
- [ ] **DoD-3 Recorded.**

## 8. Verification

```powershell
cargo test -p dedupe-chrome --lib produce
cargo test --manifest-path crates\dedupe-chrome\ui\Cargo.toml produce
```

## 9. Deferred roll

| Row | Disposition |
|---|---|
| D-0137-produce-preflight-actions | **Absorb** |
| D-0125-dead-css / pad-fallback | Remain unless a one-line CSS touch while here |
| 0119 latch | **Keep frozen** |
| 0125 canvas | **Keep frozen** |
| Last-PR comments | **Decline** |
| Fold-in opencode-M1 / AGY-137-01 | **Fold** — plain `<a>` for in-page hashes; `<A>` only for review |
| Fold-in opencode-m1 | **Fold** — `qc_gate` is Re-run QC only (no self-hash) |
| Fold-in opencode-m2 | **Fold** — unknown kind + no id → no action |
| Fold-in AGY-137-02 | **Fold** — `include_str` `id="privilege-protocol"` |
| Fold-in opencode-O1 / D-0125-dead-css | **Remain** — do not touch `app.css` unless required |
