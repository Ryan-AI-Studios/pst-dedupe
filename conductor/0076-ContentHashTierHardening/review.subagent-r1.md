# 0076 ContentHashTierHardening — Completion Review (subagent R1)

**Track:** `0076-ContentHashTierHardening`  
**Branch:** `feat/0076-content-hash-tier-hardening`  
**Verdict:** **FAIL**

See orchestrator session for full audit. Summary of blocking findings:

## P0
- `--tier1-backfill` does not merge groups (and stops counting when flag on)

## P1
- `body-recip-attach` silent no-op (accept level but never populate attach digests)
- Recipient / Tier2.5 honesty counters never incremented
- No DedupIndex ↔ group_candidates equivalence test
- Fixture refinement assertion missing
- unique-pst human summary omits grouping honesty stats

## P2
- `--identity-ignore-inline-attachments` is a no-op
- Missing §3.14 tests (allow-degenerate restore, tier1-verify, 9b–9d, help, ≥1000)
- GUI legacy scan path not on GroupingContext

## Test failure observed
- `source_rank_flips_winner_file_a_vs_a2` — from_a=17 default_a2=17 (source-rank should flip winners)

## Also green
- `cargo test -p dedup-engine --lib` 147 ok
- `cargo test -p pst-dedup-cli --lib` 83 ok
- keep_set: 11/12 (one fail above)
