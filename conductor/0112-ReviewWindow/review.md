# 0112-ReviewWindow — Review

- **Track:** `0112-ReviewWindow`
- **Branch:** `track/0112-review-window` (product) → `docs/0112-completed` (registry)
- **Registry:** **Completed**
- **Product PR:** **#115** squash-merged to `main` as `81a3aad`
- **Docs PR:** records this Completed registry (after product merge)

---

## Definition of Done (§7)

| DoD | Status | Evidence |
|---|---|---|
| **DoD-1 — Window replaces stub** | **PASS** (engineering) | `/matters/:id/review/:docId` is three panes (related \| viewer \| code); `coding-pane` token `#E8EEF2`; radios ⊥ Privilege; overlay `3` = Needs review / `p` privilege / `d` ditto. Queue route unchanged. `dedupe-desk` builds. Owner release-EXE HITL residual (below). |
| **DoD-2 — Persist + neighbors + family** | **PASS** | Host fixture `insert_family` → parent → children; `family_size==3`; propagate false vs true; Unreviewed dropout `next_id=itm_0001` / `position != 0`; encrypted; `not_found`. Queue passes filter/keyword query params. |
| **DoD-3 — Privilege type + withhold** | **PASS** | Pre-check basis → `apply_codes` → upsert; no-basis no write; final `withhold==false`; description omitted preserves existing; notes are `review_upsert_note` only. |
| **DoD-4 — Viewer honesty** | **PASS** | `cas_len` + `read_cas_prefix(2 MiB)` + `from_utf8_lossy`; Hello/World whitespace + `!HelloWorld`; 2 MiB+1 truncated; missing digest empty; Image copy names **0114**. |
| **DoD-5 — Catalog lock + CI** | **PASS** | Catalog read-first (held `open_for_read`); queue apply still forces propagate false; five new `allow-*`; `cargo test -p dedupe-chrome` **62 passed**. PR **#115** required CI green (fmt/clippy/test/audit/deny/chrome-ui/verify-parity). No production `unwrap`/`expect`; CSP unchanged. |
| **DoD-6 — Recorded** | **PASS** | Product PR **#115** / `81a3aad`. Registry **Completed**; `D-0112-review-window` closed. Unblocks **0113** / **0114**. **0117** stays Proposed. |

---

## Internal review rounds

| Round | Open | Outcome |
|---|---|---|
| 1 | **2 P2** (queue filter not passed; withhold+codes persist) | Fixed |
| 2 | Easy P3s (stub file, notes list, `priv` ident) | Fixed |

---

## Codex completion audits

| Round | Verdict | Notes |
|---|---|---|
| r1 | **FAIL** (P1 notes leak, P1 description wipe, P2 family preview, P3 stub copy) | Fixed |
| r2 | **FAIL** (P1 note as privilege_description; P2 >100 preview; P2 confirm on no-code; P2 basis leak) | Fixed |
| **r3** | **PASS** | `review.codex.r3.md` — no open P0–P3 |

**Open engineering findings > low:** none.

---

## Residuals (deferred / external)

| Item | Notes |
|---|---|
| **Owner HITL** | Release EXE + synthetic 3-doc family: Enter from queue, `1` then Enter, `p` + type, family off, Image stub, coding pane paper-blue. INC* waived. |
| **D-0026-03** | Partial chrome absorb (text + stripped HTML). Image raster remains **0114**. |
| **D-0117-queue-virtualization** | Remains Proposed (queue header/spacer, vacant lie, arrow scroll). |
| **D-0110-deny-unic** | Remains (upstream unic-* via Tauri). |
| **D-0062-codesign** | Release ops; not this track. |

---

## Pins / stack (re-verified at implement)

- `tauri` **2.x**; `leptos` **0.8** CSR; `trunk` **0.21.14** (CI `chrome-ui`)
- `SCHEMA_VERSION` **39**
- Ledger FEATURE tx `78a03ed9-2efb-4f34-8328-f054301cbd17` (hook-promoted on product commit)

---

## Conductor files requiring `git add -f`

```
conductor/0112-ReviewWindow/spec.md
conductor/0112-ReviewWindow/plan.md
conductor/0112-ReviewWindow/review.md
conductor/0112-ReviewWindow/review.codex.md
conductor/0112-ReviewWindow/review.codex.r2.md
conductor/0112-ReviewWindow/review.codex.r3.md
conductor/0112-ReviewWindow/foldin-note.md
conductor/0112-ReviewWindow/opencode-review.md
conductor/0112-ReviewWindow/agy-review.md
conductor/conductor.md
conductor/ROADMAP.md
conductor/sequencing.md
```
