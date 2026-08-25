# Track Completion Audit — 0089-UniqueEmlAttachLedger

## Verdict: FAIL

The implementation satisfies DoD-1 through DoD-5 on static review and supplied test evidence. Completion is blocked by unmet DoD-6 and insufficient end-to-end regression coverage for the production orchestration path.

## Scope Reviewed

- Branch: `feat/0089-unique-eml-attach-ledger`
- Base: `e4bbd9f`
- Working tree: 15 modified files; no untracked review artifact
- Read fully: spec.md, plan.md
- Reviewed engine DTO/event generation, CLI flags and wiring, sink/schema, Mode A handling, tests, docs, deferred records, and governance status.
- No files or Git state were modified.

## Requirement and DoD Matrix

| Requirement | Status | Evidence / Gap |
|---|---|---|
| DoD-1 — Flags and defaults | Met | main.rs defines all three flags with full defaults and threads them into UniqueEmlCliArgs. |
| Engine DTO boundary | Met | EmlAttachEvent and EmlWriteResult::attachment_events live in dedup-engine; no engine→CLI dependency. |
| Reason mapping | Met | Open/IO/cloud causes map to 0073 codes; unmapped causes map to ATTACH_UNKNOWN; ATTACH_PART_FAILED is not emitted as CSV reason. |
| DoD-2 — CSV path/schema/wiring | Met statically | Production loop maps events into AttachLedgerSink; Mode A drains soft-skip records and calls mark_promoted_winner; header reuses EXPORT_ATTACHMENTS_CSV_HEADER. |
| CSV injection safety | Met | Existing AttachLedgerRow::to_csv_line applies formula neutralization and CSV escaping. |
| DoD-3 — Row cap | Met | Shared sink cap and ATTACH_LEDGER_TRUNCATED marker behavior are used. |
| DoD-4 — Exit/fidelity/off/fail-closed | Met statically | Counters remain classification source of truth; ledger init/flush errors set report_ok=false. |
| DoD-5 — Close D-0073-eml | Met | docs/deferred.md, changelog, and operator docs mark it closed in 0089. |
| Tests prove production reachability | Partial | Unit tests cover components, but the CLI integration test asserts only the header/path and does not require an emitted failure row. |
| DoD-6 — Review/governance/ledger | Unmet | No canonical review.md; conductor remains In Progress; plan declares TX 36f4223f-… open. |

## Findings

### [P1] Track completion governance is still incomplete

Confidence: High
Requirement: DoD-6
Problem: Canonical review.md missing; conductor In Progress; FEATURE TX open. Expected mid-cycle; orchestrator finalize after engineering clean.
Deferrable: No (finalize residual)

### [P2] Core ledger wiring lacks an end-to-end regression test

Confidence: High
Requirement: DoD-2, DoD-4, Plan Phase 2
Problem: Integration test verifies export_attachments.csv exists and has the header, but does not require a soft-fail row. Component tests bypass run_unique_eml.
Correction: Add production-path test asserting emitted failure row through orchestration.
Deferrable: No

## Completeness Sweep

No blocking TODOs/stubs. ATTACH_PART_FAILED limited to pack-manifest. MIME unchanged.

## Wiring and Regression Review

Production path wired correctly on static review. off/full/basename/cap/fail-closed invariants hold statically.

## Verification Evidence

Orchestrator-reported gates passed (eml_pack 29, unique_eml 12, export_exit_0078 10, fmt, clippy). Ledgerful status unavailable in Codex sandbox.

## Deferred Candidates

None.

## Completion Decision

Do not mark completed yet. Resolve P2 production-path coverage; P1 DoD-6 is orchestrator finalize after re-review.

---

## Post-review dispositions (orchestrator)

| Finding | Disposition | Evidence |
|---|---|---|
| P1 DoD-6 governance incomplete | **Expected mid-cycle** — finalize after fresh Codex PASS | review.md / Completed / FEATURE TX commit pending |
| P2 production-path soft-fail CSV row | **Fixed** | Added `unique_eml_production_soft_fail_writes_ledger_row` in `tests/unique_eml.rs` — synthetic cloud-link PST → real `unique-eml` CLI → header + fail row (`ATTACH_CLOUD_LINK`/`ATTACH_STREAM_OPEN_FAILED`) + exit 64; observed `ok` |
