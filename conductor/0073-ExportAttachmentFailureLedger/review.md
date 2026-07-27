# Track 0073 — ExportAttachmentFailureLedger — Review

**Status:** **Completed**  
**Date:** 2026-07-26  
**Branch / PR:** `feature/0073-export-attachment-failure-ledger`  
**Final cross-model:** Codex `gpt-5.6-luna` high — **PASS WITH DEFERRED P3** (`review.codex.final.md`)

## Objective

Make every soft-failed (and policy-omitted) attachment on the unique-export path identifiable, classifiable, joinable, and remediable via `export_attachments.csv` + summary histogram.

## What shipped

| Area | Change |
|---|---|
| **Writer taxonomy** | Expanded `AttachmentFidelityKind` with stable `as_code()` SCREAMING_SNAKE strings; all former silent `attachments_failed++` go through `record_attach_event` |
| **Locus events** | `AttachmentFidelityEvent` carries source_path, folder_path, msg_nid, attach_nid, attach_index, method, size, severity |
| **Sink** | Optional `AttachEventSink` on `write_unicode_pst_streaming`; CLI uses volume-local `VolumeAttachBuffer` → commit to global on Ok only |
| **Ledger CSV** | `export_attachments.csv` via mpsc + background BufWriter; `--attach-ledger full\|summary-only\|off`; max-rows default 500k + `ATTACH_LEDGER_TRUNCATED` |
| **CSV safety** | `csv_escape_cell` formula neutralization for `=+\-@` |
| **summary.json** | Additive `attachments_failed_by_reason`, ledger path/mode/truncated/rows_written, omit counter |
| **export_messages** | Appended `attachments_failed_count` |
| **parents_only** | Materializer lists attach metadata; writer emits info omit rows (not fail) |
| **Meta list fail** | `attach_list_failed` only when `AttachMetaFailed` **and** empty attach list |
| **Residuals** | Mode C ledger-only promote; unique-eml / GUI / basename / Vec-events deferred |

## Review rounds

| Round | Reviewer | Verdict | Action |
|---|---|---|---|
| Internal #1 | explore subagent | FAIL (P2 off-mode omit clobber + DoD-17) | Fixed omit; deferred governance |
| Internal #2 | explore subagent | PASS WITH DEFERRED P3 | Proceed to Codex |
| Codex #1 | gpt-5.6-luna high | FAIL (P1×3 + P2×3) | Fixed parents_only meta list, ledger init fail-closed, volume buffer, off msg counts, MetaFailed, source_id empty |
| Codex #2 | gpt-5.6-luna high | FAIL (P2 probe double-count) | Restrict `attach_list_failed` to empty list + AttachMetaFailed |
| Codex final | gpt-5.6-luna high | **PASS WITH DEFERRED P3** | Clean final gate |

## DoD matrix (engineering)

| DoD | Result |
|---|---|
| 1 Taxonomy | Met |
| 2 Locus events | Met |
| 3 Ledger CSV + sink | Met |
| 4 Invariant | Met (committed volumes only) |
| 5 Histogram | Met |
| 6 Omit ≠ fail | Met |
| 7 Zero-byte success | Met |
| 8 Promote | Residual **D-0073-promote** |
| 9 Partial honesty | Met (`attachments_failed_count`) |
| 10 unique-eml | Residual **D-0073-eml** |
| 11 Exit honesty | Met |
| 12 CSV injection | Met |
| 13 Row cap | Met |
| 14 source_id | Met |
| 15 Docs | Met |
| 16 Tests | Met |
| 17 Recorded | Met (this file + registry Completed + deferred) |

## Deferred (docs/deferred.md)

| ID | Severity | Item |
|---|---|---|
| D-0073-promote | P1 residual | Mode A pre-write promote |
| D-0073-eml | P2 residual | unique-eml ledger parity |
| D-0073-gui | P3 | GUI attach-ledger UI |
| D-0073-basename | P3 | path redaction handoff mode |
| D-0073-vec-events | P3 | writer event Vec growth |

## Verification (orchestrator)

```text
cargo fmt --all --check                          # pass
cargo test -p pst-writer --test writer_fidelity  # 33 passed
cargo test -p pst-writer --test writer_streaming # 17 passed (pre-final)
cargo test -p pst-dedup-cli --test unique_pst    # 17 passed
cargo test -p pst-dedup-cli unique_export_report # 11 passed
cargo clippy -p pst-writer -p pst-dedup-cli -p pst-dedup-gui --all-targets -- -D warnings  # pass
# Full workspace gate run before commit/PR
```

## Completion decision

Engineering DoD met; residuals recorded; final Codex gate clean. Track **Completed**.
