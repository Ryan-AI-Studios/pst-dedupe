# 0108 PolyDegradedWinnerRisk — Review

**Status:** Completed  
**Branch:** `track/0108-poly-degraded-winner-risk`  
**Closes:** `D-0108-poly-degraded-winner-risk`  
**Does not close:** `D-0108-keepset-crc-retaint` (P3 residual — keep-set still re-taints)

## Objective delivered

Unique-pst `export_risk` keys advisory `degraded_winner_rate` on **`effective_degraded_winner_rate`** when present: poly-only `CrcSuspect` / `AttachStreamCrc` on `poly_class_crc` sources are excluded (same poly discount gate as 0099). Raw `degraded_winner_rate` stays on `inputs`. Body/attach and non-poly CRC still elevate. Attest: `degraded_winners_poly_only`. Keeps-set / Tier-2 / `max_degraded_winner_rate=0.02` unchanged.

## DoD matrix

| Item | Result | Evidence |
|---|---|---|
| **DoD-1** Policy in code | **PASS** | `poly_degraded_winner_adjustment`; Ok-branch keying; all-poly CrcSuspect-only → `ok` + `poly_class_crc_discounted`; raw 1.0 on inputs; AttachStreamCrc / CrcMismatch / unique==0 tests |
| **DoD-2** Real degrade still elevates | **PASS** | Scaled 39+2 → `effective_degraded_winner_rate=0.049>0.02`; localized uses raw; degrade reasons only when `post == Ok` |
| **DoD-3** Keep-set / Tier-2 unchanged | **PASS** | No restrip; `stats.degraded_winners` still counts CRC-tainted winners; no schema bump |
| **DoD-4** Oracle | **PASS** | Pointers for both new keys; not on `SUMMARY_ALLOWLIST_KEYS` |
| **DoD-5** Docs / deferred | **PASS** | Additive CRC/integrity rows; D-0108 closed; keepset-retaint updated once; 0099 wording; CHANGELOG |
| **DoD-6** Recorded | **PASS** | This file; registry Completed; ledger BUGFIX tx |
| **DoD-7** HITL optional | **Not run** | Optional INC* re-smoke under `output/inc0102784-post-0108/` (gitignored) |

## Review rounds

| Gate | Result |
|---|---|
| Internal review (effort 1) | **0 open** — `grok-review-c804d763.md` |
| Codex #1 (gpt-5.6-luna high) | **PASS** — `review.codex.md` (no P0–P3) |

## Gates (orchestrator)

```
cargo fmt --all --check                          → pass
cargo clippy --workspace --all-targets -- -D warnings → pass
cargo test --workspace                           → pass
ledgerful verify                                 → (see publish note)
```

## Residual

- `D-0108-keepset-crc-retaint` (P3): scan poly-clears; unique-pst keep-set re-taints. 0108 keys export_risk only.
- **0109** Still next for also-eml classify honesty.
- Series O frontend stays **0110+** — not started.
- Optional HITL: expect not `degraded_winner_rate=1.000>0.02`; effective ≈ 0.031 may still advisory.

## Publish

| Field | Value |
|---|---|
| PR | _(filled after open)_ |
| Merge SHA | _(filled after squash-merge)_ |
