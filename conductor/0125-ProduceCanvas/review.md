# 0125 — ProduceCanvas — Review

## Scope

Un-wizard Produce: all five steps stay visible (no exclusive `Show when=step==N`). Three-pane CSS `236px minmax(0,1fr) 320px`. Privilege protocol always on the left; empty 502 notes render **none on file**. 320px Stage owns Finalize. **0119** `volume_succeeded` latch frozen. `fail_if_withheld` / `require_qc_pass` unchanged. Schema stays **41**. No fake Bates, Stage snapshot, ACME ranges, or categorical log. **0126** not implemented.

## DoD matrix

| Item | Result | Evidence |
|---|---|---|
| DoD-1 Un-wizard | PASS | Five `.produce-step` bodies with `#step-1-set` … `#step-5-preflight`. No exclusive step `Show`. CSS three-pane. Step-5 tab auto-`run_qc` gone; no mount auto-QC. Pre-flight + Stage: **QC not yet run — click Re-run QC**. |
| DoD-2 Protocol + sets | PASS | Additive `protocol_*` on `produce_page` from `get_privilege_protocol`. Empty notes → `none on file`. New busy-guard; clears `bates_start` to `""`; restores DAT-only; Format `<select>` bound. Set rows = live `ProductionSetThin`. UI new fields `#[serde(default)]`. Privilege-log radio syncs from live protocol. |
| DoD-3 Stage + Finalize | PASS | 320px Stage. Finalize in Stage with the same latch + click no-op. Disabled on `qc` None / blockers / missing Bates / incomplete warn overrides. Snapshot disabled “not this track.” Export paths from Thin layout folders. Pad from Thin. Live prefix (not hardcoded `"PROD"`). |
| DoD-4 Hygiene | PASS | No new production `unwrap`/`expect`. Schema 41. 0119 latch + host privilege-in-set / empty-union tests green. Plex / `#1b3049`. UI 42 tests; `cargo test -p dedupe-chrome` 117; workspace + clippy `-D warnings`. chrome-ui CI trunk build green. |
| DoD-5 Recorded | PASS | This file; registry **Completed**; `D-0125-produce-canvas` closed. Ledger FEATURE committed on the product squash (`f94e5e98`). Owner HITL remaining (release EXE). |

## Gates

| Command | Result |
|---|---|
| `cargo fmt --all --check` | pass |
| `cargo clippy --workspace --all-targets -- -D warnings` | pass |
| `cargo test --manifest-path crates/dedupe-chrome/ui/Cargo.toml` | pass (42) |
| `cargo test -p dedupe-chrome` | pass (117, including privilege-in-set + empty-union + 0119 latch) |
| `cargo test --workspace` | pass |
| `ledgerful verify` | pass |
| CI (PR **#143**) | fmt, clippy, test, audit, deny, chrome-ui, verify-parity **green**. Bugbot skipping (does not block). |
| Final cross-model gate | **PASS**, no open >low (`review.codex.md` round 3) |

## Reviewer rounds

1. Internal: DoD-1…4 PASS. Mediums: New vs Format `<select>` desync (fixed `prop:value`); projected last Bates on entire-corpus without QC (omitted).
2. Codex r1: **FAIL** — Stage export paths hard-coded; verification not observed; DoD-5 premature. Export paths copied onto Thin from `p.body.layout`. Workspace gates run. DoD-5 declined until after merge.
3. Codex r2: **FAIL** — privilege-log radio always started as `standard` (would overwrite live `automated_metadata` on Finalize). Fixed `protocol_log_format_radio` on `produce_page` load.
4. Final gate (fresh): **PASS**. No P0–P3.

## HITL (owner)

Release chrome EXE on a synthetic matter: all five steps visible without changing a tab; Stage pane present; Finalize disabled while pre-flight blockers remain; after one successful Finalize, second click stays latched (**0119**). Protocol shows **none on file** when notes are empty. INC* unique-pst is **not** a gate. Codesign is **D-0062-codesign**.

## Residual lows (deferred)

| ID | Item |
|---|---|
| D-0125-dead-css | Dead `.produce-foot` and `.produce-steps li.active button` CSS |
| D-0125-pad-fallback | Stage pad display falls back to 6 if Thin `pad_width` is 0 |

## Publish

- Branch: `track/0125-produce-canvas`
- PR: **#143**
- Merge SHA: `1fbc22a00fc400d19766b060233e7664bb4cca2b`
