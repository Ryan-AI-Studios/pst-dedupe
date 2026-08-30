# 0111-ReviewQueueFirstPass — Review

- **Track:** `0111-ReviewQueueFirstPass`
- **Branch:** `track/0111-review-queue-first-pass` (product) → `docs/0111-completed` (registry)
- **Registry:** **Completed**
- **Product PR:** **#113** squash-merged to `main` as `3c4ca65`
- **Docs PR:** records this Completed registry (after product merge)

---

## Definition of Done (§7)

| DoD | Status | Evidence |
|---|---|---|
| **DoD-1 — Queue replaces stub** | **PASS** (engineering) | `/matters/:id/review` is first-pass queue (not 0110 stub); Unreviewed default; Lead/QC off; Continue review CTA; 0110 tabs; `dedupe-desk` builds. Owner release-EXE HITL residual (below). |
| **DoD-2 — Honesty of counts + columns** | **PASS** | Host 1k fixture: `total=1000`/`rows.len()==50`; `HashSet::is_disjoint` pages; empty/source-only zeros; uncoded=2; `family_sizes`→3; extras false/true; encrypted; Control#=`review_order`; privilege coding ≠ withhold; unknown resp → `—`. |
| **DoD-3 — Virtualization** | **PASS** (CI math) | `visible_range` top / near-bottom / overscroll / span≤36; `limit>500` rejected; UI `.queue-row` windowed. DOM ≤64 at rest on release EXE is owner HITL. |
| **DoD-4 — Saved search + keyboard + bulk** | **PASS** | Saved upsert/list; `fts_unavailable` kind; preview N=2; responsive/confidential N=0; propagate forced false / actor `chrome`; Enter/click → 0112 stub; Esc clears bulk even in fields; shortcuts skip interactive targets including anchors. |
| **DoD-5 — Tests + CI** | **PASS** | `cargo test -p dedupe-chrome` **42 passed**; fmt/clippy/workspace; PR **#113** required CI green (fmt/clippy/test/audit/deny/chrome-ui/verify-parity). No production `unwrap`/`expect`; CSP unchanged; six `allow-*`; no `fs:default`. |
| **DoD-6 — Recorded** | **PASS** | Product PR **#113** / `3c4ca65`. Registry **Completed**; `D-0111-first-pass-queue` closed. Unblocks **0112**. |

---

## Internal review rounds

| Round | Open | Outcome |
|---|---|---|
| 1 | **6** (1 bug include-family + suggestions) | Fixed |
| 2 | **2** (fetch_gen on parse Err; mount comment) | Fixed |
| Re-review | **0** | Clean |

---

## Codex completion audits

| Round | Verdict | Notes |
|---|---|---|
| r1 | **FAIL** (P1 process + 5× P2) | Chip totals; unknown resp; keyboard/controls; FTS empty; preview order — fixed |
| r2 | **FAIL** (2× P2) | Row click navigate; Esc clears bulk — fixed |
| r3 | **FAIL** (1× P2) | Anchors in shortcut gate — fixed |
| **r4** | **PASS** | `review.codex.r4.md` — no open findings |

**Open engineering findings > low:** none.

---

## Residuals (deferred / external)

| Item | Notes |
|---|---|
| **Owner HITL** | Release EXE + synthetic 1k: footer `1000 in queue`, DOM `.queue-row` ≤64, Unreviewed, `?`, Enter → 0112 stub. INC* waived. |
| **D-0110-deny-unic** | Remains (upstream unic-* via Tauri). |
| **D-0026-01** | Desk egui Load-more residual stays (chrome partial absorb only). |

---

## Pins / stack (re-verified at implement)

- `tauri` **2.x**; `leptos` **0.8** CSR; `trunk` **0.21.14** (CI `chrome-ui`)
- `SCHEMA_VERSION` **39**; `matter-search` host dep allowed this track
- Ledger FEATURE tx `e5f16c37-ab75-4f87-9535-54f2c6293c15` committed

---

## Conductor files requiring `git add -f`

```
conductor/0111-ReviewQueueFirstPass/spec.md
conductor/0111-ReviewQueueFirstPass/plan.md
conductor/0111-ReviewQueueFirstPass/review.md
conductor/0111-ReviewQueueFirstPass/review.codex.md
conductor/0111-ReviewQueueFirstPass/review.codex.r2.md
conductor/0111-ReviewQueueFirstPass/review.codex.r3.md
conductor/0111-ReviewQueueFirstPass/review.codex.r4.md
conductor/0111-ReviewQueueFirstPass/foldin-note.md
conductor/0111-ReviewQueueFirstPass/opencode-review.md
conductor/0111-ReviewQueueFirstPass/agy-review.md
conductor/conductor.md
conductor/ROADMAP.md
conductor/sequencing.md
```
