# 0132 — CliProgressPowershell — Review

## Scope

PowerShell operators can capture unique-pst progress without a wall of NativeCommandError records. Progress stays on stderr; `--json` stays a parseable stdout document. Docs-only close (plus timing-script comment). No `--progress-file`. No BCC-default.

## DoD matrix

| Item | Result | Evidence |
|---|---|---|
| DoD-1 Runbook §2b | PASS | `cmd /c` + `Start-Process`; NativeCommandError wrapping explained. Pointer from day-1 `scan --json` piped to `Set-Content`. No `&&`. |
| DoD-2 Timing script | PASS | `scripts/unique-pst-timing.ps1` `RedirectStandardError = $false` comment. JSON stdout unchanged. |
| DoD-3 JSON contract | PASS | Docs-only (plus comment); CLI routing unchanged. No `--progress-file`. |
| DoD-4 Hygiene | PASS | No bashisms in the snippet. Series workspace fmt/clippy/test (no unique-pst CLI protocol change). |
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

Owner may smoke interactive unique-pst vs `cmd /c ... 2> log` -- optional, no INC* in git. Owner INC* re-smoke is optional (not a gate). Codesign is **D-0062-codesign**.

## Residual lows (deferred)

| ID | Item |
|---|---|
| (this track) | none above low |
| D-0127-* / D-0067 | see 0127 |
| 0131 HITL | 2026-09-03 PASS -- see 0131 (RI 826→821) |
| Owner INC* re-smoke | optional, not a gate |

## Publish

- Branch: `track/0127-0132-series-u`
- PR: **#147** https://github.com/Ryan-AI-Studios/pst-dedupe/pull/147
- Merge SHA: `90827614db3b5149ecc8560132f3a0640b14eb9a` (short `9082761`)
- Commit: `track(0127-0132): unique-pst HITL residuals (depth hint, risk note, timings) (#147)`
- Ledger FEATURE tx `75d98b69-96b3-4514-889d-2c6a7aef8183` COMMITTED on the product squash
- Locks held: no BCC-default; do not raise `--max-embedded-depth` default (stays 3); identity depth 3; D-0067-embedded-depth stays open; no fourth ExportRisk enum; threshold 0.02; do not discount BODY_UNAVAILABLE; never fudge unaccounted_ms to 0; QC default stays sample; `--prefer-folder-class` stays opt-in; no `--progress-file`; JSON stays stdout
