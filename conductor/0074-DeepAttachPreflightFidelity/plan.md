# 0074 — Deep Attach Preflight & Fidelity Bridge — Plan

> **Ledger:** `ledgerful ledger start 0074-deepattachpreflightfidelity --category FEATURE --message "Budgeted deep attach preflight + fidelity bridge"`

**Status:** **Ready** — execute after reading `spec.md` (review folds 2026-07-26).

## Locks (from spec)

1. **Budgeted L2 head-probe default** — not unbounded full-read (§3.3–3.4)  
2. **Winner-only unique-pst P0**; optional scan flag (§3.2)  
3. **Shared reason strings** with 0073 / 0065 (§3.5)  
4. **attach_fail_rate** escalates preflight (§3.6)  
5. **Degrade integrity** + **max_peer_probes_per_group** (§3.7 / §3.7.1)  
6. **Honesty** — truncated/coverage; not export guarantee (§3.8)  
7. **Bounded LRU sticky handles** (`max_open_psts`); progress + cancel (§3.9.1)  
8. **Per-attach wall-clock** (`max_probe_time_ms`) (§3.4.1)  
9. **Cache (if any):** key includes level (+ size/mtime); no L1→L2 poison (§3.10)  
10. **parents_only** skips probe  
11. **No auto-ScanPST** / no source mutation  

## Phase 0 — Design freeze → DoD foundation

- [ ] Confirm 0073 reason string table (or freeze jointly if 0073 not merged)  
- [ ] Choose default: unique-pst deep probe opt-in vs default-on (document in review)  
- [ ] Pin default budgets (`max_attaches`, `max_probe_bytes`, `per_attach_max_bytes`, `max_attach_fail_rate`)  
- [ ] Inventory materialize 64 KiB path + `stream_available`  
- [ ] `ledgerful ledger start 0074-deepattachpreflightfidelity --category FEATURE --message "…"`  
- [ ] Optional: `ledgerful scan --impact`  

## Phase 1 — Probe engine + reasons → DoD-1, DoD-4, DoD-8, DoD-11

- [ ] Additive `IntegrityReason` variants + `as_str` / parse / `reason_from_pst_error`  
- [ ] `probe_attach_stream(level, budgets) -> Result/ProbeOutcome` (chunked discard)  
- [ ] Per-attach `max_probe_time_ms` abort  
- [ ] **Bounded LRU** PST handle cache (`max_open_psts`, default 32)  
- [ ] Unit tests: open fail, read fail, no fat Vec, budget truncate, timeout, LRU cap  

## Phase 2 — Preflight math → DoD-5

- [ ] Extend `PreflightInputs` / `PreflightReport` (or nested `attach_probe`)  
- [ ] Threshold `max_attach_fail_rate`; reason `attach_stream_fail_rate_exceeded`  
- [ ] Pure unit tests for escalation matrix  
- [ ] Strict-mode behavior documented + tested  

## Phase 3 — Materialize / unique-pst winner path → DoD-2, DoD-6, DoD-7, DoD-9, DoD-12

- [ ] Wire winner-only deep probe behind flag  
- [ ] Merge reasons into fidelity; fix stream_available on fail  
- [ ] Prefer clean peer via existing `fidelity_rank` (fixture test)  
- [ ] **max_peer_probes_per_group** (default 3); counter when capped  
- [ ] Progress + cancel hooks on unique-pst  
- [ ] Skip when parents_only / no-attachments  

## Phase 4 — Optional scan path + cache → DoD-3, DoD-13

- [ ] `--deep-attach-preflight` on scan  
- [ ] Same budgets/level flags  
- [ ] Soft: in-process cache with **level + size/mtime** key (§3.10)  

## Phase 5 — Docs → DoD-10, DoD-15

- [ ] Scan + unique-pst help / `docs/unique-pst-export.md`  
- [ ] Integrity / preflight doc: attach_probe fields, coverage honesty, FD/budget knobs  
- [ ] Re-export vs ScanPST-on-copy guidance  
- [ ] Cross-link 0073 ledger residual + 0077 noise  

## Phase 6 — Gate + finalize → DoD-14, DoD-16

- [ ] Targeted tests (§3.11)  
- [ ] `cargo clippy` on touched crates `-D warnings`  
- [ ] Full workspace gate before commit  
- [ ] `review.md` + **D-0074-*** residuals  
- [ ] Registries → **Completed**  
- [ ] `ledgerful ledger commit <tx-id> --summary "…" --reason "…"`  

## Suggested order

1. Reasons + pure preflight rates  
2. Probe engine (L2 + budgets)  
3. Materialize/unique-pst winner wire  
4. Scan flag  
5. Docs + review  

## Handoff notes

- **Do not** full-read every attach by default.  
- **Do not** claim export-clean from L2 success.  
- **Do not** auto-ScanPST or mutate sources.  
- **Do not** diverge reason strings from 0073.  
- **Do not** reintroduce per-page CRC log floods.  
- **Do not** cache open handles for every custodian PST unbounded.  
- **Do not** probe unlimited peers in one keep-set group.  
- **Do not** serve L1 cache hits for L2 requests.  
- Rollback: leave flags off — existing scan/materialize unchanged.
