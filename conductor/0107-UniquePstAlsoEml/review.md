# 0107 UniquePstAlsoEml — Review

**Status:** Completed  
**Branch:** `track/0107-unique-pst-also-eml`  
**HEAD (pre-merge):** `323277a`  
**Closes:** `D-0071-also-eml`  
**Does not close:** `D-0067-embedded-depth` (matter/Relativity children residual)

## Objective delivered

`unique-pst --also-eml <dir>` writes a unique-EML pack from the **same keep-set winners** as the unique-PST (shared `write_eml_pack_from_keep_set`; no second scan; no `run_unique_eml`). Nested MIME reuses 0106. Combined exit uses 0078 precedence `130 > 1 > 65 > 64 > 0`. Summary always-present `also_eml_*` keys; oracle strips only `also_eml_out`.

## DoD matrix

| Item | Result | Evidence |
|---|---|---|
| **DoD-1** Co-export same keep-set; guards; cancel isolation; 0078 precedence; Mode A drain; real scan/scan_ok | **PASS** | `unique_eml_cmd.rs` helper; `unique_pst_cmd.rs` wire; parent-of-`--out` + volume-sibling guards; cancel PST skip / also-eml quarantine |
| **DoD-2** Summary fields + tests | **PASS** | Six `also_eml_*` without `skip_serializing_if`; `unique_pst_also_eml` **13** tests; callback invert; method-5 Subject; Mode A ledger |
| **DoD-3** Docs + D-0071 closed | **PASS** | `docs/unique-pst-export.md`, `docs/unique-eml-import.md`, CHANGELOG; deferred row closed |
| **DoD-4** review.md + Completed + ledger | **PASS** (this file; registry updated at publish; FEATURE txs committed on implement) | Ledger txs on branch commits |

## Review rounds

| Gate | Result |
|---|---|
| Internal review round 1 | 7 open (1 bug, 6 suggestions) → fixed in `069e6d7` |
| Internal re-review | **0 open** |
| Codex #1 | FAIL (risk gate regression, hard-fail summary, …) |
| Codex R1 fixes | `95da222` |
| Codex #2 | FAIL (parent-of-out, summary-write Err, tests) |
| Codex R2 fixes | `eee75b8` |
| Codex #3 | FAIL (cancel+Err loses 130; quarantine collision) |
| Codex R3 fixes | `67c2643` |
| Codex #4 | FAIL (hard-fail provenance scan_ok/counts) |
| Codex R4 fixes | `323277a` |
| Codex #5 (fresh) | **PASS** — `review.codex.md` / `review.codex.round5.md` |

## Commits (feature branch)

1. `ae43f75` — co-export from same keep-set  
2. `069e6d7` — internal review fixes  
3. `95da222` — Codex R1 honesty  
4. `eee75b8` — parent-out guard + summary fail-closed  
5. `67c2643` — cancel over helper Err  
6. `323277a` — hard-fail scan_ok + counts  

## Residual

- Frontend / Hermes Series O stays **0108+** (not started).  
- `D-0067-embedded-depth` remains open.  
- Cancel-during-also-eml full I/O inject remains unit/helper-covered; cancel-during-PST production path tested.  
- No HITL required (spec).

## Publish

| Field | Value |
|---|---|
| PR | [#104](https://github.com/Ryan-AI-Studios/pst-dedupe/pull/104) |
| Merge SHA | `339dfa07729c8119e8516eab285a26144deac523` |

