# Track Completion Audit — 0113-ProduceChecklist

## Verdict: FAIL

## Scope Reviewed

Reviewed `spec.md`, `plan.md`, the working tree on `track/0113-produce-checklist` versus `origin/main`, and the relevant Chrome, matter-core, matter-qc, matter-produce, UI, permissions, schema, and governance changes.

The branch has no committed delta from `origin/main`; the implementation is present as working-tree changes, including untracked files. DoD-6 was treated as orchestrator-owned.

## Requirement and DoD Matrix

| Area | Result |
|---|---|
| Five-step Chrome produce route | Pass |
| Default responsive, non-withheld, family-inclusive set | Pass on normal UI path; direct-ID boundary issue below |
| Family ordering and withheld-child hard block | Pass |
| QC pack/scope binding and stale membership gate | Pass for membership |
| Stored QC findings and warning/error enforcement | Fail: fail-open report loading |
| Produce parameters and DAT-only volume layout | Pass |
| Privilege-log produced/withheld union | Pass |
| Bates assignment and review display | Partial: QC order is not preserved |
| Permissions, encryption handling, schema v39 | Pass |
| No OPT/IMAGES/TIFF/process-runner integration | Pass |
| DoD-1 | Pass |
| DoD-2 | Pass based on reported tests and code inspection |
| DoD-3 | Fail due stored-findings fallback and warning-run identity |
| DoD-4 | Partial: ordering persistence gap |
| DoD-5 | Pass based on reported gates and inspection |
| DoD-6 | Orchestrator-owned; not used as an engineering failure |

## Findings

### [P1] Missing or malformed stored QC findings fail open

- Requirement: DoD-3; stored QC findings must control finalization, with no silent fallbacks.
- Location: [produce.rs:320](C:/dev/Dedupe/crates/dedupe-chrome/src/produce.rs:320), [produce.rs:348](C:/dev/Dedupe/crates/dedupe-chrome/src/produce.rs:348)
- Problem: `load_findings_csv` returns an empty findings list when `findings.csv` is missing or unreadable, silently skips malformed records, and substitutes empty values for missing columns. `current_engine_findings` also returns an empty list when the report path is absent.
- Failure scenario: A passed QC database row remains fresh, but its report directory or `findings.csv` is deleted/corrupted. Finalization sees no warnings or errors and can proceed without required warning overrides.
- Required correction: Make report loading fallible and fail closed. Reject missing, unreadable, malformed, or structurally invalid reports before produce can start.
- Verification gap: Existing tests prove that UI `last_findings` is ignored, but do not prove missing/corrupt stored findings block production.
- Deferrable: No.

### [P1] Caller-supplied item IDs bypass the requested FilterSpec

- Requirement: Host must re-resolve the produce selection from the FilterSpec at QC and produce time.
- Location: [produce.rs:182](C:/dev/Dedupe/crates/dedupe-chrome/src/produce.rs:182)
- Problem: Any non-empty `item_ids` argument is treated as authoritative. `order_ids_family_together` validates matter membership/order but does not enforce the responsive, non-withheld, or selected scope predicates.
- Failure scenario: A direct Tauri caller supplies an item outside the review corpus or outside the requested filter. QC and production operate on that item because the filter is bypassed.
- Required correction: Always resolve the authoritative candidate set from the FilterSpec, or explicitly validate that supplied IDs exactly equal the resolved set before proceeding. Keep direct IDs only as a controlled test seam if needed.
- Deferrable: No.

### [P2] Produce can use a different order from the QC run

- Requirement: Finalization must use the same ordered IDs used by QC.
- Locations: [produce.rs:541](C:/dev/Dedupe/crates/dedupe-chrome/src/produce.rs:541), [produce.rs:68](C:/dev/Dedupe/crates/dedupe-chrome/ui/src/pages/produce.rs:68)
- Problem: QC returns `ordered_ids`, but the UI does not send them to `produce_start`, and the host does not persist them. Freshness fingerprints sort IDs, so membership changes in ordering alone do not make QC stale.
- Failure scenario: Review order changes after QC while membership remains identical. The gate passes, but production recomputes a new order and may assign different Bates numbers.
- Required correction: Persist or submit the QC run’s ordered IDs and require finalization to reuse that exact order, or make ordering changes stale the QC run.
- Deferrable: No.

### [P2] Warning overrides omit QC run identity and can survive QC reruns

- Requirement: Warning override payloads must include the current QC run identity and audit evidence.
- Locations: [produce.rs:54](C:/dev/Dedupe/crates/dedupe-chrome/src/produce.rs:54), [produce.rs:600](C:/dev/Dedupe/crates/dedupe-chrome/src/produce.rs:600), [produce.rs:118](C:/dev/Dedupe/crates/dedupe-chrome/ui/src/pages/produce.rs:118)
- Problem: `WarningOverride` contains only `recorded_by`, `reason`, `rule_id`, and `item_id`. The UI retains overrides when a new QC run or profile is selected, and the host validates only the rule/item key.
- Failure scenario: An operator records an override, reruns QC, and finalizes using the old override without evidence tying it to the current QC run.
- Required correction: Include `qc_run_id` in the QC response and override payload, validate it against the current stored run, and clear or re-key overrides when QC is rerun or the profile/source changes.
- Deferrable: No.

## Completeness Sweep

- No `ProduceStub`, TODO placeholder, `todo!`, `unimplemented!`, or obvious no-op path remains in the scoped implementation.
- No Chrome dependency on `process-runner`, `dedupe-desk`, or `zpdf`.
- No production `unwrap()`/`expect()` found in the scoped Chrome/matter-core implementation; test-only uses remain.
- DAT-only output is wired; no working `OPT`, `IMAGES`, TIFF, or PDF production path was found.
- Schema remains version 39.
- Privilege-log blank-description blocking and produced ∪ withheld-in-scope union are implemented.
- Permissions and encrypted-matter open paths are wired.
- No 0114/0115/0117/0118 product implementation was found in the scoped changes.

## Wiring and Regression Review

The normal UI path reaches all three host commands and uses the intended QC and production engines. Queue, review-window, overview, and home-summary integrations are connected.

The principal regressions are at the host trust boundaries: direct IDs can override selection resolution, stored QC findings are not fail-closed, and Bates ordering is not bound to the QC order. Warning override provenance is also incomplete.

## Verification Evidence

Observed:

- `git diff --check` passed.
- Static inspection confirmed schema v39, DAT-only constraints, permissions, dependency boundaries, and no scoped production unwrap/expect.
- Ledgerful status/doctor could not open its database in this read-only environment.
- AI-brains preflight was unavailable because the vault key was not configured.

Reported by the implementation context, not independently rerun:

- `cargo test -p dedupe-chrome`
- `cargo test -p matter-core --test produce_helpers`
- Focused clippy with `-D warnings`

These reported gates do not cover the four findings above, particularly missing/corrupt QC reports and order preservation.

## Deferred Candidates

None. The identified items affect selection authorization, QC enforcement, Bates determinism, or audit provenance and should not be deferred as P3 work.

## Completion Decision

FAIL. The track is substantially implemented and the primary happy path is wired, but completion is blocked by two P1 enforcement failures and two P2 correctness/provenance gaps.