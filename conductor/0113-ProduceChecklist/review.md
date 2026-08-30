# 0113-ProduceChecklist — Review

- **Track:** `0113-ProduceChecklist`
- **Branch:** `track/0113-produce-checklist` (product) → `docs/0113-completed` (registry)
- **Registry:** **Completed**
- **Product PR:** **#117** squash-merged to `main` as `f192b2d`
- **Docs PR:** records this Completed registry (after product merge)

---

## Definition of Done (§7)

| DoD | Status | Evidence |
|---|---|---|
| **DoD-1 — Wizard replaces stub** | **PASS** (engineering) | `/matters/:id/produce` is the five-step checklist (Set / Number / Format / Burn / Pre-flight). Format greys TIFF/OPT (**0115**). Burn copy names **0114**. Four 0110 tabs still work. Queue + 0112 window still work. `dedupe-desk` builds. No `process-runner` on `dedupe-chrome`. Owner release-EXE HITL residual (below). |
| **DoD-2 — Default set + privilege-in-set** | **PASS** | Host tempfile: parent+2 children; withheld child stays in default `include_family` count; first QC returns Error `withheld_in_selection` **and** chrome `uncoded_in_set`; start fails; after coding + clearing withhold, QC pass then start succeeds. `fail_if_withheld` true; QC `scope=item_ids` and same pack. |
| **DoD-3 — Checklist gate** | **PASS** | Warning payload requires `recorded_by` + `reason` + `qc_run_id`; empty reason refused; Error findings not overridable; empty selection blocker; membership drift → `qc_gate` stale, never silent re-QC. Stored `findings.csv` fail-closed (missing/empty/invalid header). |
| **DoD-4 — Volume + Bates + chip** | **PASS** | `DATA/load.dat` (BOM + `BEGBATES==ENDBATES==CONTROL_NUMBER`), `NATIVES/`, `TEXT/`, `privilege-log.csv`. No `IMAGES/` / `IMAGE.opt`. Produced log ControlNumber is Bates; withheld-in-scope row ControlNumber = item_id. Window Bates from `latest_control_number`. `matter_overview.produced` ≥ 1. Parent Bates < child; first-seen family keeps lower Bates than later family. |
| **DoD-5 — Helpers + CI** | **PASS** | `list_item_ids_filtered` / `order_ids_family_together` / `count_produced_items` covered. New `allow-produce-*`. Encrypted → `encrypted`. `cargo test -p dedupe-chrome` produce tests **10 passed**. PR **#117** required CI green (fmt/clippy/test/audit/deny/chrome-ui/verify-parity). No production `unwrap`/`expect`. CSP unchanged. SCHEMA_VERSION **39**. |
| **DoD-6 — Recorded** | **PASS** | Product PR **#117** / `f192b2d`. Registry **Completed**; `D-0113-produce-checklist` closed. **0114** / **0115** / **0116** / **0117** / **0118** stay as they are. |

---

## Internal review rounds

| Round | Open | Outcome |
|---|---|---|
| 1 | **2 P2** (privilege-log dry-run wrote audit; `last_findings` could skip warning gate) + easy P3s (Bates auto-fill, unused log format, entire-corpus count) | Fixed: read-only blank count; ignore UI findings cache; persist protocol format; no Bates auto-fill |

---

## Codex completion audits

| Round | Verdict | Notes |
|---|---|---|
| r1 | **FAIL** (P1 fail-open findings.csv; P1 item_ids bypass FilterSpec; P2 QC order not reused; P2 override missing `qc_run_id`) | Fixed |
| r2 | **FAIL** (P1 empty/header-invalid findings.csv still fail-open when stored counts are 0) | Fixed |
| **r3** | **PASS** | `review.codex.r3.md` — no open P0–P3 |

**Open engineering findings > low:** none.

---

## Residuals (deferred / external)

| Item | Notes |
|---|---|
| **Owner HITL** | Release EXE + synthetic 3-doc family: withheld blocker visible in red, warning override requires text, clean DAT volume, home chip numeric. INC* waived. |
| **D-0113-long-job** | Blocking `join_worker` for QC/produce. DoD fixture is small. Owner **0116**. |
| **D-0040-04** | Partial: `privilege-log.csv` at volume root. `PRIVILEGE/` folder residual. |
| **D-0031-09** | Partial: chrome volume log uses Bates map. Desk/CLI still item_id. |
| **D-0114** / **D-0115** / **D-0116** / **D-0117** / **D-0118** | Stay as they are. |
| **D-0110-deny-unic** | Remains (upstream unic-* via Tauri). |
| **D-0062-codesign** | Release ops; not this track. |

---

## Pins / stack (re-verified at implement)

- `tauri` **2.x**; `leptos` **0.8** CSR; `trunk` **0.21.14** (CI `chrome-ui`)
- `SCHEMA_VERSION` **39**
- Ledger FEATURE tx `3f0c8b0f-1d2e-4a5d-8ba6-6384cb2e6209` (hook-promoted on product commit)

---

## Conductor files requiring `git add -f`

```
conductor/0113-ProduceChecklist/spec.md
conductor/0113-ProduceChecklist/plan.md
conductor/0113-ProduceChecklist/review.md
conductor/0113-ProduceChecklist/review.codex.md
conductor/0113-ProduceChecklist/review.codex.r2.md
conductor/0113-ProduceChecklist/review.codex.r3.md
conductor/conductor.md
conductor/ROADMAP.md
conductor/sequencing.md
```
