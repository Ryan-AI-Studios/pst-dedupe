# Track Completion Audit — 0089-UniqueEmlAttachLedger (internal R1)

## Verdict: PASS

## Scope Reviewed

| Item | Detail |
|---|---|
| Track | `0089-UniqueEmlAttachLedger` |
| Track dir | `C:\dev\Dedupe\conductor\0089-UniqueEmlAttachLedger` |
| Execution repo | `C:\dev\Dedupe` |
| Branch (handoff) | `feat/0089-unique-eml-attach-ledger` |
| Spec / plan | Full `spec.md` §7 DoD + `plan.md` Phases 0–3; `implementation-notes.md` |
| Inspected | `crates/dedup-engine/src/eml_pack.rs` (`EmlAttachEvent`, `map_eml_attach_fail_reason`, soft-fail emit); `crates/pst-dedup-cli/src/unique_eml_cmd.rs` (ledger init, Mode A, write-loop enqueue, fail-closed, summary, unit tests); `crates/pst-dedup-cli/src/main.rs` UniqueEml flags; `unique_export_report.rs` sink APIs; `tests/unique_eml.rs` + `tests/export_exit_0078.rs`; `docs/deferred.md` D-0073-eml; `docs/unique-eml-import.md`; `docs/unique-pst-export.md` residual line |
| Compared | `unique_pst_cmd.rs` Mode A soft-skip + `mark_promoted_winner`; `AttachLedgerSink::ingest` vs `enqueue_soft_skip_row` |
| Not done this session | No source edits beyond this review file; no Git mutations; **cargo fmt/clippy/test not executed here** (orchestrator-reported gates below) |

Implemented surface (static):

- **dedup-engine:** `EmlAttachEvent` on `EmlWriteResult`; sole soft-fail site in `prepare_attachments` emits events; `map_eml_attach_fail_reason` → 0073 codes / `ATTACH_UNKNOWN` (never `ATTACH_PART_FAILED` as ledger reason).
- **pst-dedup-cli:** UniqueEml flags `--attach-ledger` / `--attach-ledger-max-rows` / `--ledger-path-mode`; pack-root `AttachLedgerSink`; Mode A drain + `mark_promoted_winner`; write-loop maps events → rows; fail-closed init/flush → `report_ok=false`; summary ledger fields.
- **Docs / deferred:** `D-0073-eml` closed; operator unique-eml + unique-pst residual lines updated.

---

## Requirement and DoD Matrix

| Requirement | Met/Partial/Unmet | Evidence | Tests | Gap |
|---|---|---|---|---|
| **DoD-1 — Flags** `--attach-ledger`, `--attach-ledger-max-rows`, `--ledger-path-mode`; default `full` | **Met** | `main.rs` UniqueEml args: `default_value = "full"`, `DEFAULT_ATTACH_LEDGER_MAX_ROWS`, `parse_attach_ledger_mode` / `parse_ledger_path_mode`; threaded into `UniqueEmlCliArgs` | Integration CLI `--attach-ledger full\|off`; clap defaults | None |
| **DoD-2 — CSV** soft-fail + Mode A soft-skip → `{out}/export_attachments.csv`; identical header; `mark_promoted_winner`; injection-safe | **Met** | Sink opened on `--out`; header = `EXPORT_ATTACHMENTS_CSV_HEADER`; Mode A `mark_promoted_winner` + `enqueue_soft_skip_row` for `soft_skip_attach_records`; write-loop maps `attachment_events`; rows use `AttachLedgerRow::to_csv_line` → `csv_escape_cell` / formula neutralize | Unit: soft-fail header+row, Mode A winner_promoted; integration: pack-root header identity; sink CSV injection covered in `unique_export_report` | None material |
| **DoD-3 — Cap** row-cap + truncated marker matches 0073 | **Met** | Same `AttachLedgerSink::enqueue_soft_skip_row` cap path as unique-pst soft-skip (`ATTACH_LEDGER_TRUNCATED`) | Unit `attach_ledger_row_cap_truncated_marker` | None |
| **DoD-4 — Exit** exit 64 / fidelity / counters with ledger on or off; init fail fail-closed | **Met** | Classify uses `attach_parts_failed` (counters); ledger Off still classifies from counters; init Err when mode≠Off sets `ledger_init_error` → `report_ok=false` → `REPORT_WRITE_FAILED` | `unique_eml_ledger_off_still_exit_64_from_counters`; `attach_ledger_init_fail_full_fail_closed`; integration off=no CSV | None |
| **DoD-5 — Deferred** `D-0073-eml` closed | **Met** | `docs/deferred.md` status **closed / 0089**; CHANGELOG; `unique-pst-export.md` unique-eml residual line; no open code comments claiming residual | N/A | `D-0073-gui` remains (out of scope) |
| **DoD-6 — Recorded** `review.md`; conductor Completed; ledger TX | **Unmet** (finalize residual) | Plan Phase 3 open; conductor still **In Progress**; TX `36f4223f-…` open per plan | — | Orchestrator after review |
| Reason map never drops / never ledger-`ATTACH_PART_FAILED` | **Met** | `map_eml_attach_fail_reason`: cloud→`ATTACH_CLOUD_LINK`; Io/open→`ATTACH_STREAM_OPEN_FAILED`; else `ATTACH_UNKNOWN`; rustdoc forbids pack-manifest code | Engine soft-fail + cloud-link unit tests | Operator one-liner for `ATTACH_UNKNOWN` — see Fix-Now |
| Wiring: write-path via `enqueue_soft_skip_row` vs `msg_fail_counts` | **Met (not a DoD gap)** | See Wiring | — | unique-eml has no `export_messages.csv` column fed by `msg_fail_counts` |

---

## Findings

None blocking. No P0–P2. Engineering DoD-1…DoD-5 met on static evidence + orchestrator package tests.

### Fix-now recommendations (easy P3; not deferred)

#### [P3] F-001 Operator docs omit `ATTACH_UNKNOWN` fallback

Confidence: High  
Requirement: Spec §2.4 “generic **documented** code”; Phase 0 mapping table  
Location: `C:\dev\Dedupe\docs\unique-eml-import.md` (flags/layout only); mapping lives in `implementation-notes.md` + `eml_pack.rs` rustdoc  
Problem: Unmapped / `PathBudget` soft-fails map to `ATTACH_UNKNOWN`, documented in track notes and code, but not in the operator unique-eml guide (or unique-pst reason→action table).  
Evidence: `map_eml_attach_fail_reason` returns `ATTACH_UNKNOWN`; `docs/unique-eml-import.md` has no reason-code note.  
Failure scenario: Operator sees unfamiliar `ATTACH_UNKNOWN` in CSV and assumes a second dialect or silent drop. Behavior is correct; docs gap only.  
Correction: One sentence under unique-eml attach-ledger section: unmapped EML soft-fails → `ATTACH_UNKNOWN` (never `ATTACH_PART_FAILED` as CSV reason).  
Verification: Doc-only review.  
Deferrable: **No** — easy fix-now (do not open a deferred ID).

#### [P3] F-002 Human summary omits attach-ledger path

Confidence: High  
Requirement: DoD-2 operator discoverability (secondary; JSON/`summary.json` already expose fields)  
Location: `C:\dev\Dedupe\crates\pst-dedup-cli\src\unique_eml_cmd.rs` human-summary block (~700–750)  
Problem: Non-JSON run prints attach fail counts and `summary.json` path but not `{out}/export_attachments.csv` when mode=`full`.  
Evidence: Grep of human println block — no `attachment_ledger` / `export_attachments` line; `UniqueEmlSummaryOut` / JSON do include ledger fields.  
Failure scenario: Operator without `--json` may miss the CSV beside the pack.  
Correction: When `attachment_ledger` is `Some`, print the pack-root CSV path (mirror unique-pst report echo).  
Verification: Smoke unique-eml without `--json` after a soft-fail fixture / unit redirect.  
Deferrable: **No** — easy fix-now.

---

## Completeness Sweep

| Search / check | Result |
|---|---|
| Open `D-0073-eml` claims in code | **None** in `unique_eml_cmd.rs` / `eml_pack.rs`; deferred row **closed / 0089** |
| Residual operator claims | `unique-eml-import.md` documents flags + CSV path; `unique-pst-export.md` residual: unique-eml ledger **closed in 0089** |
| `TODO` / `FIXME` / `todo!` / `unimplemented!` in 0089 paths | **None** blocking |
| Placeholders / fake success | Fail-closed init test plants dir-as-CSV blocker → real `AttachLedgerSink::new` Err; soft-fail uses `NullAttachStreamSource` production write path |
| `ATTACH_PART_FAILED` as ledger reason | Used only as pack-manifest / `merge_pack_degraded` aggregate reason — **not** in `map_eml_attach_fail_reason` / CSV `reason_code` |
| `error_detail` on `EmlAttachEvent` | Diagnostic on DTO; **no** 0073 CSV column (header lock) — intentional |
| Engine→CLI dependency | None; DTO stays in `dedup-engine` |
| Production `unwrap`/`expect` in new cmd paths | Test-only `expect`; production uses `unwrap_or_else` for path absolutize fallback only |
| Plan Phase 3 finalize | Open — orchestrator (`review.md`, conductor Completed, ledger commit) |

---

## Wiring and Regression Review

```text
unique-eml flags (main)
  → UniqueEmlCliArgs
  → finalize_with_materialize_opts (Mode A soft_skip_attach_records)
  → AttachLedgerSink::new(--out)   [fail-closed if mode≠Off && Err]
  → mark_promoted_winner(promoted winners)
  → enqueue_soft_skip_row(soft_skip records, winner_promoted=true)
  → write_canonical_eml → EmlWriteResult.attachment_events
  → attach_ledger_row_from_eml_event + enqueue_soft_skip_row
  → ledger.finish() → summary attachment_ledger* / histogram
  → classify_export(attach_failed_total = manifest.attach_parts_failed, report_ok)
```

**Write-path `enqueue_soft_skip_row` vs `ingest` / `msg_fail_counts`:**  
unique-pst write-path uses `AttachEventSink` → `ingest`, which updates `msg_fail_counts` for `export_messages.attachments_failed_count` in all ledger modes. unique-eml has **no** `export_messages.csv` and classifies from `attach_parts_failed` counters (spec lock: ledger additive; counters remain classify source of truth). Using `enqueue_soft_skip_row` for write-path events still updates `failed_by_reason` (when mode≠Off) and writes CSV rows (mode=full) — matching Mode A soft-skip and implementation notes. **Not a DoD gap for unique-eml.**

**Mode A:** Soft-skip drain + `mark_promoted_winner` before write loop; write-fail rows set `winner_promoted` from `promoted_winner_loci` — mirrors `unique_pst_cmd`.

**Fail-closed:** Init failure does not continue with a silent `None` when mode≠Off; flush errors also clear `report_ok`.

**Header identity:** Constant reuse; integration + unit assert equality with `EXPORT_ATTACHMENTS_CSV_HEADER`.

**Regression risk:** None observed on exit-64 counter path with ledger off; MIME layout unchanged (events additive on soft-fail only).

---

## Verification Evidence

| Class | Detail |
|---|---|
| **Observed now** | Static read/grep of listed files; no cargo commands run in this reviewer session |
| **Reported by orchestrator** | `cargo test -p dedup-engine -- eml_pack` → **29 passed**; `cargo test -p pst-dedup-cli --test unique_eml` → **12 passed**; `cargo test -p pst-dedup-cli --test export_exit_0078` → **10 passed** |
| **Not verifiable here** | `cargo fmt --all --check`; `cargo clippy -p dedup-engine -p pst-dedup-cli --all-targets -- -D warnings`; `ledgerful verify`; full workspace test |
| **Recommended before finalize** | Run pending fmt/clippy + `ledgerful verify`; then DoD-6 recording |

Source-level test map (acceptance):

| Plan / DoD test | Location |
|---|---|
| Soft-fail → CSV + header | `unique_eml_cmd::soft_fail_eml_event_writes_export_attachments_csv_header`; `unique_eml::unique_eml_attach_ledger_csv_header_at_pack_root` |
| Mode A promoted + loser rows | `unique_eml_cmd::mode_a_soft_skip_and_promoted_winner_rows` |
| Row-cap truncated marker | `unique_eml_cmd::attach_ledger_row_cap_truncated_marker` |
| Ledger off → exit 64 from counters | `export_exit_0078::unique_eml_ledger_off_still_exit_64_from_counters`; integration `unique_eml_attach_ledger_off_no_csv` |
| Ledger init fail fail-closed | `unique_eml_cmd::attach_ledger_init_fail_full_fail_closed` |
| Engine event + reason map | `eml_pack` soft-fail / cloud-link unit tests |

---

## Deferred Candidates

| ID / proposal | Deferrable? | Notes |
|---|---|---|
| F-001 `ATTACH_UNKNOWN` operator note | **No** | Fix-now docs |
| F-002 Human ledger path line | **No** | Fix-now polish |
| `D-0073-gui` | Already open | Out of scope (spec §4) |
| Unify pst-writer `AttachEventSink` ↔ `EmlAttachEvent` | Spec declined | Future opportunity only |

No new deferred.md entries proposed.

---

## Completion Decision

**Verdict: PASS** — engineering DoD-1…DoD-5 are met on code, docs, and orchestrator package-test evidence. Write-path use of `enqueue_soft_skip_row` is intentional and not a unique-eml DoD gap (`msg_fail_counts` / `export_messages` are unique-pst-only).

Before marking conductor **Completed** / committing ledger TX (DoD-6):

1. Apply easy fix-now P3s F-001 and F-002 (optional for strict DoD text; recommended for operator polish).
2. Record pending fmt/clippy (+ `ledgerful verify`) results.
3. Write canonical `review.md`; set conductor Completed; commit TX `36f4223f-8c7f-4824-84ae-c8af743d81ca`.

---

## Post-review fix-now disposition

| Finding | Disposition | Notes |
|---|---|---|
| F-001 Operator docs omit `ATTACH_UNKNOWN` | **Already fixed** | `docs/unique-eml-import.md` §6 honesty: unmapped soft-fail → `ATTACH_UNKNOWN` (row never dropped); CSV uses 0073 taxonomy; pack-manifest `ATTACH_PART_FAILED` is not a CSV `reason_code`. |
| F-002 Human summary omits attach-ledger path | **Already fixed** | `unique_eml_cmd.rs` human-summary prints `attach_ledger: {out}/export_attachments.csv` when `attachment_ledger` is `Some` (mode=full). |
