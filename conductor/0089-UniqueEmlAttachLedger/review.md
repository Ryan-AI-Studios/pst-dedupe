# 0089 — Unique-EML Attach Ledger Parity — Completion review

**Track:** 0089-UniqueEmlAttachLedger  
**Status:** Completed (engineering + governance)  
**Branch:** `feat/0089-unique-eml-attach-ledger`  
**Closes:** D-0073-eml  
**Ledger TX:** `36f4223f-8c7f-4824-84ae-c8af743d81ca`

## Scope

Bring `unique-eml` attach soft-skip / failure reporting to full CSV ledger parity with unique-pst’s 0073 `export_attachments.csv`: same header (`EXPORT_ATTACHMENTS_CSV_HEADER`), pack-root locus (`--out/export_attachments.csv`), reason taxonomy, row-cap, CSV-injection safety, and Mode A `winner_promoted` / soft-skip rows — via `EmlAttachEvent` DTO in `dedup-engine` + CLI `AttachLedgerSink` (no engine→CLI dependency).

## Reviewers / rounds

| Round | Reviewer | Verdict | Notes |
|---|---|---|---|
| Implementer | engineering | landed | Flags, DTO, sink, Mode A, tests, docs; D-0073-eml closed |
| Internal #1 | general-purpose | **PASS** | DoD-1..5 Met; DoD-6 finalize residual; easy P3 F-001/F-002 fixed |
| Codex #1 (r1) | gpt-5.6-luna high | **FAIL** | P2: production-path soft-fail CSV e2e missing; DoD-6 unmet (expected mid-cycle) |
| Fix | orchestrator | — | Added `unique_eml_production_soft_fail_writes_ledger_row` |
| Codex **final** | gpt-5.6-luna high | **PASS** | Findings none; DoD-1..5 Met; DoD-6 finalize residual for orchestrator |
| Finalize | orchestrator | — | This file; board Completed; FEATURE TX commit |

## What shipped

- `dedup-engine` `EmlAttachEvent` + `EmlWriteResult.attachment_events`; soft-fail emit + `map_eml_attach_fail_reason` (never CSV `ATTACH_PART_FAILED`)
- `pst-dedup-cli` UniqueEml flags `--attach-ledger` / `--attach-ledger-max-rows` / `--ledger-path-mode` (default `full`)
- Pack-root `AttachLedgerSink`; write-loop map events → rows; Mode A soft-skip drain + `mark_promoted_winner`
- Fail-closed ledger init/flush; exit 64 / fidelity still from counters (ledger off included)
- Docs + `docs/deferred.md` close **D-0073-eml**

## DoD matrix

| DoD | Status | Evidence |
|---|---|---|
| **1** Flags / defaults | **Met** | UniqueEml CLI flags; default `full` |
| **2** CSV + Mode A | **Met** | Pack-root CSV; identical header; Mode A promote + soft-skip; production e2e |
| **3** Row cap | **Met** | Shared sink + truncated marker test |
| **4** Exit / fail-closed | **Met** | Counters classify; off still exit 64; init fail fail-closed |
| **5** Deferred | **Met** | D-0073-eml closed / 0089 |
| **6** Recorded | **Met** | this file; conductor **Completed**; FEATURE TX committed |

## Gates (observed)

```text
cargo test -p dedup-engine -- eml_pack              → 29 passed
cargo test -p pst-dedup-cli -- unique_eml           → 13 passed (incl. production soft-fail ledger e2e)
cargo test -p pst-dedup-cli -- export_exit_0078     → 10 passed
cargo fmt --all --check                             → ok
cargo clippy -p dedup-engine -p pst-dedup-cli --all-targets -- -D warnings → ok
```

## Residuals / dispositions

| Item | Disposition |
|---|---|
| Codex r1 P2 production soft-fail e2e | **Fixed** before final PASS |
| DoD-6 at Codex final | **Closed** in this finalize |
| Track deferred entries | **None** from this track |
| `D-0073-gui` | **Out of scope** (elsewhere; not opened/closed here) |

## Findings closed this track

- Internal: no blocking P0–P2; easy P3 docs/summary path fixed
- Codex r1 P2 → fixed; Codex final: no findings
