# 0102-ExportOracleInputsAttest — Review

## Status

Engineering DoD-1..3 met. Publish in progress; PR# / merge SHA filled when available.

## Scope

- Branch: `track/0102-ExportOracleInputsAttest`
- Product commit: `c8f3128`
- Base: `origin/main` @ `11e455f`
- Locked fix: remove `"inputs"` from `SUMMARY_ALLOWLIST_KEYS`; keep root `/inputs` blanking; keep four 0099 attest pointers.

## DoD matrix

| DoD | Result | Evidence |
|---|---|---|
| DoD-1 Strip honesty | **PASS** | `"inputs"` removed from allowlist; root blanking comment; pointers unchanged in `compare_integrity_counters` |
| DoD-2 Tests | **PASS** | Six synthetic unit tests in `export_oracle.rs`; `cargo test -p pst-dedup-cli --lib export_oracle` → 9 passed |
| DoD-3 Docs | **PASS** | `docs/unique-pst-export.md`, module doc, `unique_pst` env-gate comment, CHANGELOG, `D-0099-oracle-inputs-attest` closed |
| DoD-4 Recorded | **PASS** (this file + registry Completed + ledger linked on `c8f3128`) | Codex r1 FAIL was only missing DoD-4 artifacts |

## Gates

| Gate | Result |
|---|---|
| `cargo fmt --all --check` | ok |
| `cargo clippy --workspace --all-targets -- -D warnings` | ok |
| `cargo test -p pst-dedup-cli --lib export_oracle` | 9 passed |
| `cargo test --workspace` | ok (pre-commit hygiene on `c8f3128`; re-run for publish) |
| `ledgerful verify --scope fast --auto-index` | **failed** once: `Step timed out after 300s: cargo test --workspace` (cold/contended). Fallback: verify.commands equivalents above. |
| Ledger | tx `a967a2d1-389e-4a0a-b7af-62fd4b4d4b92` linked at commit; `ledger status --compact` → 0 pending |

## Reviewer rounds

| Round | Verdict | Notes |
|---|---|---|
| Internal `/implement` | clean | 0 open issues |
| Codex r1 (`review.codex.md`) | **FAIL** | Sole finding: DoD-4 not finalized (expected mid-publish). Engineering DoD Met. Disposition: **Validated process gap → fixed here**. |
| Codex r2 | *(pending after this governance commit)* | Must be PASS / PASS WITH DEFERRED P3 |

## Deferred lows

None from this track.

## Publish

| Field | Value |
|---|---|
| PR | *(pending)* |
| Merge SHA | *(pending)* |
| Next Series P | 0103 Proposed (not started) |
