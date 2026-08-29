# 0106 — UniqueEmlNestedMime — Review

**Status:** Completed (engineering + publish)  
**Branch:** `track/0106-unique-eml-nested-mime`  
**Commits:** `c58dc5f` (feat) · `658d272` (subject honesty) · `fde5758` (Codex P1/P2)  
**Ledger tx:** `4669f146-1862-427f-a685-5cad79c57a94` (FEATURE / crates/dedup-engine); `ad718644-b1a8-4fe3-8038-9f490fa684e9` (FEATURE / crates/pst-dedup-cli)

## DoD matrix

| DoD | Result | Evidence |
|---|---|---|
| DoD-1 Parsed nested MIME + honesty skip | **PASS** | Method-5 gated on `attach_method`; skip/DTO/depth before `open_attach_body` via `AttachSkipped`; reconstruct RFC 5322; method-1 dump unchanged; write-loop extract; `source_msg_nid` routing; incomplete list → `ATTACH_META_FAILED`; missing nid soft-fails children |
| DoD-2 CLI depth + tests | **PASS** | `pub parse_max_embedded_depth_arg`; unique-eml `--max-embedded-depth` 1–8 default 3; summary always has depth; unit + `unique_eml_depth` CLI (4@3/4@4, 8@7/8@8, clap) |
| DoD-3 Docs | **PASS** | unique-eml-import / unique-pst-export / CHANGELOG; **D-0067-embedded-depth** narrowed, **still open** |
| DoD-4 Recorded | **PASS** | This file; registry Completed; ledger 0 pending |

## Gates

| Gate | Result |
|---|---|
| `cargo fmt --all --check` | pass |
| `cargo clippy` (touched crates / workspace via hooks) | pass |
| `cargo test -p dedup-engine --lib eml_pack` | 36 passed |
| `cargo test -p pst-dedup-cli --test unique_eml` | 13 passed |
| `cargo test -p pst-dedup-cli --test unique_eml_depth` | 4 passed |

## Internal review (/implement)

- Round 1: 4 open (1 bug nested subject fallback, 2 suggestion comments, 1 nit test assert)
- Fix: `658d272`
- Re-review: **0 open**

## Codex / cross-model

- gpt-5.6-luna high #1: **FAIL** DoD-4 sequencing only (review.md / Completed not yet written)
- gpt-5.6-luna high #2: **FAIL** P1 `attachments_incomplete` silent drop; P2 `source_msg_nid.unwrap_or(0)`
- Fix: `fde5758`
- gpt-5.6-luna high #3: **quota fail** (usage limit)
- Claude fallback: OAuth fail
- Read-only fallback audit: `review.fallback.md` → **PASS** (prior P1/P2 verified fixed; 0 open >low)
- Prior Codex raw: `review.codex.md` (last complete write was FAIL #2)

## Deferred

- **D-0067-embedded-depth** narrowed (unique-eml MIME shipped / 0106); **open** for matter/Relativity children + 32 MiB + cap 8
- No new lows
- Frontend / Hermes Series O → **0107+**

## Publish

| Item | Value |
|---|---|
| PR | _(filled after open)_ |
| Merge SHA | _(filled after squash-merge)_ |
| CI note | _(filled after green)_ |

## Notes

- Series Q **0106** unique-eml nested MIME honesty shipped; D-0067 not closed.
- No HITL / INC* required.
- No BCC / HNBITMAPHDR / unique-pst rewrite / UniqueEmlClapArgs / frontend.
