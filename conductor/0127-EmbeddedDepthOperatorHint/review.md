# 0127 — EmbeddedDepthOperatorHint — Review

## Scope

Operator disclosure when unique-pst / unique-eml hit the configured `--max-embedded-depth` cap: stderr names the flag and the **configured** cap (not a hardcoded 3). Hint keys on materializer/writer depth-limit events (`embedded_depth_limit_hits`), not the attach-ledger histogram. Default stays **3**. Identity `MAX_EMBEDDED_IDENTITY_DEPTH` stays **3**. `D-0067-embedded-depth` stays open. No BCC-default. Do not raise the default.

## DoD matrix

| Item | Result | Evidence |
|---|---|---|
| DoD-1 Depth fixtures | PASS | `unique_pst_depth` + `unique_eml_depth` assert `--max-embedded-depth=3` and `=7` (eml: once). Hint keys `embedded_depth_limit_hits`, not attach-ledger histogram. |
| DoD-2 Help + runbook | PASS | clap help unique-pst + unique-eml; runbook nested-msg 2026-09-02 pack; `default_value_t = 3` still. |
| DoD-3 unique-eml Err path | PASS | stderr via `emit_unique_eml_depth_limit_hint` in `write_eml_pack_from_keep_set_inner` BEFORE manifest/summary fail so late Err still discloses; `run_unique_eml` must not emit again. `D-0067-embedded-depth` still open. |
| DoD-4 Tests | PASS | targeted `unique_pst_depth` / `unique_eml_depth` + workspace fmt/clippy/test. |
| DoD-5 Recorded | PASS | This file; registry Completed; CHANGELOG; ledger FEATURE committed on the product squash (`75d98b69-96b3-4514-889d-2c6a7aef8183`, merge `9082761`). |

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

2026-09-03 operator-local INC* split (packs gitignored). Depth **8**: exit 0, `ATTACH_DEPTH_LIMIT` empty. Default **3**: exit **64**, stderr `--max-embedded-depth=3` once, histogram 4. Never commit INC* PSTs. Codesign is **D-0062-codesign**.

## Residual lows (deferred)

| ID | Item |
|---|---|
| D-0067-embedded-depth | remains (matter children) |

`D-0127-eml-err-stderr-assert` and `D-0127-also-eml-dual-hint` closed in hygiene after PR **#147** (`unique_eml_depth::late_manifest_err_still_emits_depth_hint`; also-eml `emit_depth_limit_hint: false`).

## Publish

- Branch: `track/0127-0132-series-u`
- PR: **#147** https://github.com/Ryan-AI-Studios/pst-dedupe/pull/147
- Merge SHA: `90827614db3b5149ecc8560132f3a0640b14eb9a` (short `9082761`)
- Commit: `track(0127-0132): unique-pst HITL residuals (depth hint, risk note, timings) (#147)`
- Ledger FEATURE tx `75d98b69-96b3-4514-889d-2c6a7aef8183` COMMITTED on the product squash
- Locks held: no BCC-default; do not raise `--max-embedded-depth` default (stays 3); identity depth 3; D-0067-embedded-depth stays open; no fourth ExportRisk enum; threshold 0.02; do not discount BODY_UNAVAILABLE; never fudge unaccounted_ms to 0; QC default stays sample; `--prefer-folder-class` stays opt-in; no `--progress-file`; JSON stays stdout
