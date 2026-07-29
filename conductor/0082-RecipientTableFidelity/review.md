# Track 0082 — Recipient Table Fidelity — Review

**Status:** Engineering complete; Codex final gate: see `review.codex-final.md`  
**Branch:** `feat/0082-recipient-table-fidelity`  
**Ledger tx:** `93a5dc74-8119-41b3-ad6a-9c20126a010a` (FEATURE)

## Scope

Ship real MAPI recipient tables end-to-end on the unique-export path: read structured recipients (SMTP + EX/LegacyExchangeDN + `PidTagRecipientType`), use them for Tier-2.5 identity, write per-message recipient TCs into production unique-PSTs (template **0x692**, 14 MUST columns + optional `PidTagSmtpAddress`), BCC write opt-in with suppress ledger, zero-recip anomaly telemetry, and `retryable` on summary JSON.

## MS-PST citations

- Recipient Table: https://learn.microsoft.com/en-us/openspecs/office_file_formats/ms-pst/0e6d7ebd-c850-4772-ba9d-f5a642c9ff85 (access 2026-07-29)
- Recipient Table Template NID **`0x692`**, 14 MUST columns: https://learn.microsoft.com/en-us/openspecs/office_file_formats/ms-pst/bb069b2b-80ad-46d5-b86f-33487d16bf0c (access 2026-07-29)

## DoD matrix (engineering)

| DoD | Result | Evidence |
|---|---|---|
| 1 Reader | Met | `messaging/recipient.rs` list_recipients; soft-fail; flags UNSENT; tests + empty Display* no invent |
| 2 Writer | Met | Template 0x692, 15 cols (14 MUST + Smtp), always empty-or-filled TC; writer_fidelity |
| 3 BCC write | Met | default off; `--include-bcc-recipients`; tests on/off |
| 4 Pipeline | Met | materialize → CanonicalMessage.recipients → WriteMessage |
| 5 Identity | Met | SMTP→EX DN→display; EX-only merge; typed EX /CN=; empty-key fallback |
| 6 Contract | Met | `recipient_table` Preserved; BCC DroppedByDesign unless flag |
| 7 QC | Met | source table compare; include_bcc plumbing; tests both matrix cells |
| 8 retryable | Met | write_io before permanent reasons; boundary unit tests |
| 9 bcc_suppressed | Met | CSV + summary count + tests |
| 10 zero-recip | Met | counter; draft/missing flags skip; no new export_risk |
| 11 docs/deferred | Met | fidelity matrix, export, runbook, deferred closes |
| 12 deps | Met | no Cargo bumps |
| 13 gates | Met | fmt, clippy -D warnings, test workspace, deny (orchestrator) |
| 14 recorded | Met | this review.md; board Completed; ledger commit |

## Review rounds

1. **Internal DoD review:** FAIL only DoD-13/14 process (implementation DoD-1..12 met).
2. **Internal correctness:** FAIL P1 QC ignore include_bcc; P2 has_bcc/identity empty-key/tests → fixed.
3. **Internal re-review:** PASS WITH DEFERRED P3 (clean-room include_bcc summary) → fixed via `ExportSection.include_bcc_recipients`.
4. **Codex luna high:** FAIL — write_io classification, typed EX telemetry, missing empty-table test, bak pollution → fixed.
5. **Codex final:** PASS / PASS WITH DEFERRED P3 (see `review.codex-final.md`).

## Deferred dispositions (§2.6)

| ID | Disposition |
|---|---|
| D-0080-recipient-table | **closed / 0082** |
| D-0076-recipient-table | **closed / 0082** |
| D-0068-04 recipient half | **closed / 0082** (named-prop residual remains) |
| D-0018-03 | **closed / 0082** reader; matter extract-pst Display-only residual |
| D-0080-bcc-policy | **decided / 0082** (opt-in write + suppress ledger) |
| D-0078-retryable | **closed / 0082** |
| D-0073-promote / D-0073-eml / D-0080-cloud / D-0079-key | declined / remain open |

## Residual P3 (non-blocking)

- GUI has no BCC checkbox (CLI-first; wizard defaults false) — in scope decline.
- Matter extract-pst still Display* participants (D-0018-03 residual).

## Gate evidence (orchestrator)

```
cargo fmt --all --check                          PASS
cargo clippy --workspace --all-targets -- -D warnings  PASS
cargo test --workspace                           PASS
cargo deny check                                 PASS
```

## Hygiene note

ScanPST test stub previously wrote `%~dpn3.bak` when `%3` was the `-no` token, polluting `crates/pst-dedup-cli/-no.bak`. Stub now only writes bak next to the PST path (`%~2`).
