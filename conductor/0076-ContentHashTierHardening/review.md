# 0076 ContentHashTierHardening — Review (canonical)

**Track:** 0076-ContentHashTierHardening  
**Branch:** `feat/0076-content-hash-tier-hardening`  
**Status:** **Completed** — Codex final **PASS WITH DEFERRED P3**; PR/CI next 

## Scope

Identity binding hardening: character-clamped Tier-2 hash (panic fix), unread/degenerate Tier-2 ineligibility, cross-MID block, bind-time `BoundBy`, opt-in Tier 2.5 (`off|body|body-recip`; `body-recip-attach` rejected → D-0076-attach-content), `--dedupe-scope per-source` (closes D-0075-scope), `--tier1-verify`, `--tier1-backfill` (keep-set/unique only; scan/dups reject), inline MAPI ignore, honesty stats, docs.

## Reviewers / rounds

| Round | Reviewer | Verdict |
|---|---|---|
| subagent-r1 | internal explore | **FAIL** (backfill no-op, silent attach level, missing tests) |
| fix-r1 | implementer | P0/P1 addressed |
| subagent-r2 | internal explore | **PASS WITH DEFERRED P3** |
| codex r1 | gpt-5.6-luna high | **FAIL** (dups/GUI/backfill/divergence/empty-body) |
| fix-r2 | implementer | F-0076-01..07 fixed |
| codex r2 | gpt-5.6-luna high | **FAIL** (backfill×per-source; stale BoundBy) |
| fix-r3 | orchestrator | per-source partition + BoundBy reclassify after merge |
| codex final | gpt-5.6-luna high | **PASS WITH DEFERRED P3** |

## DoD summary

| DoD | Status |
|---|---|
| 1 GroupingContext | Met (CLI + GUI worker + attach_probe) |
| 2 Char clamp | Met |
| 3 Unreadable/degenerate | Met (empty clean body binds) |
| 4 Cross-MID | Met |
| 5 BoundBy | Met (member_tier deleted; backfill reclassifies) |
| 6 Strong hash body/body-recip | Met |
| 6b attach-content | Deferred honest reject **D-0076-attach-content** |
| 6c Recipient honesty | Met (counters + docs + D-0076-recipient-table) |
| 6d Inline | Met (MAPI flags) |
| 7 tier1 divergent + verify | Met (always report, optionally split) |
| 8 per-source | Met; D-0075-scope closed |
| 9 tier1-backfill | Met on keep-set/unique; scan/dups reject |
| 10 Flags + Desk checkbox | Met |
| 11 Refinement | Synthetic + ASPOSE golden green; fixture-wide baseline residual P3 |
| 12 Equivalence | Met (shuffle × contexts; backfill keep-set only documented) |
| 13 Compat | Met (CSV prefix, keep_set_v1 additive) |
| 14 Source immutability | Met (integration tests) |
| 15 Perf | Recorded below (fixture-scale) |
| 16 Docs | Met |
| 17 Full gate | Targeted + workspace tests green before PR |
| 18 Governance | This review + deferred rows |

## Performance (fixture-scale, debug binary)

| Command | Wall |
|---|---|
| `scan fixtures/aspose_outlook.pst --json` | **51 ms** |
| `scan … --strong-content-hash body --json` | **66 ms** (~+29% on 17-msg fixture; absolute delta 15 ms; not multi-GB proof → **D-0076-operator-perf**) |

Default path adds no extra I/O. Tiny fixtures make % noisy; hard ceiling +10% is for multi-GB operator residual.

## Gates observed (orchestrator)

- `cargo test -p dedup-engine --lib` — 154 ok  
- `cargo test -p pst-dedup-cli --lib` — 85 ok  
- `cargo test -p pst-dedup-cli --test keep_set` — 12 ok (ASPOSE golden ok)  
- `cargo test -p pst-dedup-cli --test unique_pst` — 18 ok  
- `cargo test -p pst-dedup-cli --test unique_eml` — 10 ok  
- `cargo test -p pst-dedup-cli --test scan_integrity` — 11 ok  
- `cargo test --workspace` — ok (post unique_eml fix)  
- `cargo clippy` targeted packages — ok  
- `cargo fmt --all` — ok  

## Deferred (docs/deferred.md)

D-0076-attach-content, recipient-table, inline residual, default-v2, bulk-class, custodian-map, gui enums, operator-perf, normalize-parity. **D-0075-scope closed.**

## Completion decision

**PASS WITH DEFERRED P3** (Codex final). Engineering DoD met; residuals in `docs/deferred.md`. Proceed PR + CI + squash.
