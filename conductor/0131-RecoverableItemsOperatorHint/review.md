# 0131 — RecoverableItemsOperatorHint — Review

## Scope

Golden-flow docs for Recoverable Items / Purges winners: unique-pst keeps those copies unless the operator passes `--prefer-folder-class`. Flag stays **opt-in**. Hint source string names the flag and has no newline. Do not default-on. Do not change source-rank / first_seen. Do not treat RI as a defect. No BCC-default.

## DoD matrix

| Item | Result | Evidence |
|---|---|---|
| DoD-1 Runbook | PASS | Runbook `--prefer-folder-class` opt-in; prefers Sent/live over RI/Purges; INC*-class dumps can keep ~20% RI winners without the flag. unique-pst vs unique-eml hint channels documented. |
| DoD-2 Ranking + hint | PASS | `keepset.rs` `winners_from_recoverable_signal_only` asserts `--prefer-folder-class` and no newline. Flag stays opt-in. Default ranking unchanged. |
| DoD-3 Owner HITL | PASS | 2026-09-03 operator-local INC* split. `--prefer-folder-class` moved RI winners **826 → 821** (5 MID groups had a live peer). Unique stayed **4055**. Flag still opt-in. Never commit INC* PSTs. |
| DoD-4 Tests | PASS | targeted `dedup-engine` recoverable tests; workspace fmt/clippy/test. |
| DoD-5 Recorded | PASS | This file; registry Completed; CHANGELOG; ledger FEATURE committed on the product squash (`75d98b69-96b3-4514-889d-2c6a7aef8183`, merge `9082761`). Docs-class work in the same squash. |

## Gates

| Command | Result |
|---|---|
| `cargo fmt --all --check` | pass |
| `cargo clippy --workspace --all-targets -- -D warnings` | pass |
| `cargo test --workspace` | pass |
| CI (PR **#147**) | fmt, clippy, test (19m4s), audit, deny, chrome-ui, verify-parity **green**. Bugbot skipping (does not block). |
| Final cross-model gate | **PASS**, 0 findings (`conductor/0127-EmbeddedDepthOperatorHint/review.codex.round2.md`) |

## Reviewer rounds

1. Internal: engineering DoD met. Easy P3: helper labeled CRC-only as BODY_UNAVAILABLE -> fixed (`body_unavailable_winners == 0` -> None).
2. Codex round 1: **FAIL** -- 3 P2s, then fixed: (1) note assigned before also-eml combine, (2) unique-eml hint skipped on late Err, (3) note omitted numeric keyed rate.
3. Internal re-review: **PASS**, no >low.
4. Codex round 2 (`conductor/0127-EmbeddedDepthOperatorHint/review.codex.round2.md`): **PASS**, 0 findings. Final cross-model gate for Series U.

## HITL (owner)

2026-09-03 Desktop `INC0102784.pst` + `INC0102784-2.pst` (gitignored packs under `output/inc0102784-hygiene-*`). `--prefer-folder-class` on the same flags as the depth-8 baseline: exit 0, fidelity complete, 4055/4055. RI winners **826 → 821**. Most RI copies have no live-mailbox peer, so they stay. Hint then names 821. Codesign is **D-0062-codesign**.

## Residual lows (deferred)

| ID | Item |
|---|---|
| D-0067-embedded-depth | remains (matter children) |

## Publish

- Branch: `track/0127-0132-series-u`
- PR: **#147** https://github.com/Ryan-AI-Studios/pst-dedupe/pull/147
- Merge SHA: `90827614db3b5149ecc8560132f3a0640b14eb9a` (short `9082761`)
- Commit: `track(0127-0132): unique-pst HITL residuals (depth hint, risk note, timings) (#147)`
- Ledger FEATURE tx `75d98b69-96b3-4514-889d-2c6a7aef8183` COMMITTED on the product squash
- Locks held: no BCC-default; do not raise `--max-embedded-depth` default (stays 3); identity depth 3; D-0067-embedded-depth stays open; no fourth ExportRisk enum; threshold 0.02; do not discount BODY_UNAVAILABLE; never fudge unaccounted_ms to 0; QC default stays sample; `--prefer-folder-class` stays opt-in; no `--progress-file`; JSON stays stdout
