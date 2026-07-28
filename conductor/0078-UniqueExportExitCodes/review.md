# Track 0078 — UniqueExportExitCodes — Completion Review

## Verdict: **PASS WITH DEFERRED P3**

Cross-model final gate: Codex `gpt-5.6-luna` high → `review.codex.final.md` (**PASS WITH DEFERRED P3**).  
Internal: `review.subagent-r1.md` → fix → `review.subagent-r2.md` → Codex FAIL rounds → fix → final clean.

## Scope

Automation exit contract for unique export: `run → Result<CliExit>`, pure `classify_export`, exit **64** (partial fidelity) / **65** (opt-in risk gate) / **130** (cancel), cancel quarantine, self-locating JSON (`fidelity` / `exit_code` / `exit_reason` / `artifact_state` / `summary_path`), unique-eml data-path attach counters (narrow D-0073-eml), keep-set contract.

Base: `main@3d693e5` (post-0077). Branch: `track/0078-unique-export-exit-codes`.

## Reviewers / rounds

| Round | Reviewer | Verdict |
|---|---|---|
| Internal r1 | subagent | PASS WITH DEFERRED P3 (easy P3s fixed) |
| Internal r2 | subagent | PASS WITH DEFERRED P3 |
| Codex r1 | gpt-5.6-luna high | **FAIL** — P1 summary contract; P2 silent skip; P2 quarantine collision |
| Fix | implementer | keep-set/unique-eml self-locating + fail-closed; quarantine stamp collision; tests |
| Codex r2 | gpt-5.6-luna high | **FAIL** — keep-set stdout-only + fail-open residual; attach production proof |
| Fix | orchestrator | always-write keep-set summary; fail-closed report; production `write_canonical_eml` attach→64 test |
| Codex final | gpt-5.6-luna high | **PASS WITH DEFERRED P3** |

## Gates (observed)

```
cargo fmt --all --check                          OK
cargo clippy --workspace --all-targets -- -D warnings  OK
cargo test --workspace                           OK
cargo test -p pst-dedup-cli --test export_exit_0078    8 passed
cargo test -p pst-dedup-cli --lib export_outcome       16 passed
```

## Key implementation

- `export_outcome.rs`: `ExportFidelity`, `ArtifactState`, `RiskGate`, `classify_export` (cumulative reasons; cancel outranks)
- `CliExit`: `PartialFidelity=64`, `ExportRiskBlocked=65`, `Cancelled=130`; **0–5 frozen**
- `run → Result<CliExit>`; no `process::exit`
- unique-pst: classify, quarantine (`{name}.cancelled-{secs}-{millis}[_N].partial`), JSON fields
- Flags: `--fail-on-partial-fidelity` (default on), `--allow-partial-fidelity`, `--fail-on-export-risk` (default off)
- unique-eml: attach counters → classify; `{out}/summary.json` fail-closed
- keep-set: always `keep_set_summary.json` (stdout-only anchors next to first input); fail-closed write

## Deferred (already in `docs/deferred.md`)

| ID | Item |
|---|---|
| D-0073-eml | Full unique-eml attach ledger CSV (narrowed: counters only) |
| D-0045-02 | Cross-process cancel (0078 only makes in-process 130 observable) |
| D-0078-retryable | `retryable: bool` JSON (not a new exit code) |
| D-0078-gui | Desk fidelity/exit UI (fields present; rich UI residual) |
| (P3) | Process-level exit-65 E2E; multi-volume mid-write cancel→retry E2E |

## Completion decision

Engineering DoD met. Locks held (refinement-only, 0–5 frozen, quarantine never delete, JSON truth). Mark **Completed** after PR CI green + squash merge.
