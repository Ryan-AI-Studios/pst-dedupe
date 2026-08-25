# Track Completion Audit — 0090-EmbeddedMsgContentHash

## Verdict: PASS

## Scope Reviewed

Engineering implementation on `feat/0090-embedded-msg-content-hash` for bounded `embedded-msg-hash/v1` under `--strong-content-hash body-recip-attach` (method-5 subnode + method-1 rfc822). Closes **D-0086-embedded-email-hash**; leaves **D-0067-embedded-depth** open. **Not Relativity dedupe parity.**

Reviewers / rounds:

| Round | Reviewer | Result |
|---|---|---|
| Internal r1 | explore | PASS WITH EASY P3 (depth assert, stream wrapper, byte charge, missing-body) |
| Internal fixes | implement | Addressed |
| Codex r1 | gpt-5.6-luna high | FAIL (budgets, strict tables, rfc822 semantics, table order, stats) |
| Codex fixes | implement | Addressed |
| Codex r2 | gpt-5.6-luna high | FAIL (charge-before-children, rfc822 child budgets, null bid_data) |
| Codex fixes | implement | Addressed |
| Codex r3 | gpt-5.6-luna high | FAIL (post-materialize body, depth-count order) |
| Codex fixes | implement | Addressed |
| Codex r4 | gpt-5.6-luna high | FAIL (raw subnode alloc before budget) |
| Codex fixes | implement | `block_payload_len_hint` + `load_pc_from_bids_with_body_budget` |
| Codex r5 | gpt-5.6-luna high | **PASS** — no P0–P3 findings |

Canonical Codex artifact: `review.codex.md` (r5 PASS).

## Requirement and DoD Matrix

| DoD | Status | Evidence |
|---|---|---|
| DoD-1 Behavior | Met | method-5 + rfc822 → `embedded-msg-hash/v1` via CLI digest fill |
| DoD-2 Budgets | Met | depth/count/byte admission; body size peek before materialize; fail-closed sentinels |
| DoD-3 Tests | Met | `embedded_msg_hash_0090` (9); `attach_content_hash` lib (16); reader embedded + PC budget units |
| DoD-4 Honesty | Met | docs not-Relativity; `strong_hash_embedded_*` QC counters; optional real-PST smoke noted |
| DoD-5 Deferred | Met | D-0086 closed; D-0067 open |
| DoD-6 Recorded | Met | this file; conductor Completed; ledger TX commit |

## Findings / dispositions

No residual Codex findings. No deferred.md adds from this track (no hard lows).

## Verification Evidence (orchestrator)

- `cargo test -p pst-reader --lib` — pass (incl. `block_payload_len_hint`, budgeted PC load)
- `cargo test -p dedup-engine -- embedded` — pass
- `cargo test -p pst-dedup-cli --test embedded_msg_hash_0090` — 9 passed
- `cargo test -p pst-dedup-cli --lib attach_content_hash` — 16 passed
- `cargo test -p pst-dedup-cli --test attach_content_0086` — 4 passed
- `cargo fmt --all --check` — ok
- `cargo clippy -p pst-reader -p dedup-engine -p pst-dedup-cli --all-targets -- -D warnings` — ok

## Optional operator-local smoke

CI uses writer method-5 / rfc822 fixtures. Optional: scan a real PST with nested embeds under `--strong-content-hash body-recip-attach` and confirm `strong_hash_embedded_parsed` / `_unparsed` / `_depth_limit` in JSON grouping stats.

## Completion Decision

**Completed.** Engineering DoD-1..6 met; Codex r5 PASS; ready to ship.
