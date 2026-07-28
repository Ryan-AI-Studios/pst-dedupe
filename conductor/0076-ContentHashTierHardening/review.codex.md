# Track Completion Audit — 0076-ContentHashTierHardening

## Verdict

**FAIL — not completion-ready.** No P0 findings, but several P1 correctness and wiring gaps remain.

## Scope

Read completely:

- `conductor/0076-ContentHashTierHardening/spec.md`
- `conductor/0076-ContentHashTierHardening/plan.md`

Audited implementation, tests, CLI/GUI wiring, docs, deferred records, governance files, and working-tree state. No files or Git state were modified.

## DoD Matrix

| DoD | Status | Audit result |
|---|---|---|
| 1 Context threading | Partial | CLI scan/keep-set/unique paths wired; `dups` and legacy GUI bypass it. |
| 2 Character clamp | Met | Char clamp, subject guards, script tests, dead GUI setting removal present. |
| 3 Tier-2 eligibility | Partial | Core keep-set/CLI logic exists; clean empty bodies are misclassified; GUI bypasses it. |
| 4 Cross-MID guard | Partial | Core grouping exists; `dups` and GUI do not use it. |
| 5 BoundBy/provenance | Partial | Core keep-set/CLI implementation exists; not end-to-end across GUI. |
| 6 Strong identity | Partial | `off|body|body-recip` exists; attach-content is deferred; attribution issues remain. |
| 6b Attach-content | Partial | CLI rejection and deferred record exist; final review decision is not completed. |
| 6c Recipient honesty | Partial | Counters exist, but attribution is heuristic and required coverage is incomplete. |
| 6d Inline attachments | Partial | MAPI detection and CLI flag are wired; GUI/`dups` paths bypass them. |
| 7 Tier-1 divergence | Partial | Divergence is not reported when `tier1-verify` splits the group. |
| 8 Per-source scope | Partial | Main grouping paths work; `dups` ignores the context. |
| 9 Tier-1 backfill | Unmet | Keep-set post-pass exists; streaming `DedupIndex` neither merges nor reports candidates. |
| 10 CLI/Desk surfaces | Partial | Flags parse, but `dups` silently ignores them. |
| 11 Refinement/golden proof | Unmet | No fixture baselines or winner golden are checked in. |
| 12 Index/group equivalence | Unmet | Test coverage omits strong identity, backfill, verification, inline, and combined contexts; backfill semantics differ. |
| 13 Compatibility artifacts | Partial | Additive JSON/CSV compatibility exists; required help snapshot evidence is absent. |
| 14 PST immutability | Not independently verified | No executed source-PST SHA evidence available. |
| 15 Performance | Unmet | No fixture timings recorded; `review.md` still says “fill on gate.” |
| 16 Documentation | Met | Identity/binding documentation and Relativity divergences are present. |
| 17 Full verification | Not independently verified | Only orchestrator-reported targeted gates available. |
| 18 Review/governance | Unmet | `review.md` is a scaffold; conductor/sequencing still mark 0076 In Progress; Ledgerful unavailable. |

## Findings

### [P1] F-0076-01 — `dups` accepts advertised flags but discards them

- **Requirement:** DoD-1, DoD-8, DoD-9, DoD-10.
- **Location:** `crates/pst-dedup-cli/src/main.rs:1545-1572`
- **Problem:** `cmd_dups` constructs `ScanOptions` with `grouping: Default::default()`. Strong hashing, per-source scope, tier1 verification, backfill, cross-MID/degenerate escapes, and inline-ignore are therefore no-ops for `dups`.
- **Impact:** Operators can receive successful output that does not reflect the selected policy.
- **Correction:** Build the same `GroupingContext` used by `scan`, and add behavior tests for every flag.
- **Deferrable:** No.

### [P1] F-0076-02 — Legacy GUI worker bypasses all 0076 safeguards

- **Requirement:** DoD-1, DoD-3, DoD-4, DoD-6, DoD-8.
- **Location:** `crates/pst-dedup-gui/src/app.rs:271`; `crates/pst-dedup-gui/src/worker.rs:87,207-233`
- **Problem:** The reachable GUI worker uses `DedupIndex::with_capacity_and_tier2`, v1-only hashing, and classic `check_and_insert`.
- **Impact:** GUI scans do not enforce unreadable/degenerate guards, cross-MID blocking, strong identity, scope, inline handling, or grouping statistics.
- **Correction:** Route the worker through the shared `GroupingContext`/full `IndexItem` path, or explicitly remove/disable the legacy scan path.
- **Deferrable:** No.

### [P1] F-0076-03 — `DedupIndex` and keep-set grouping disagree for backfill

- **Requirement:** DoD-9 and DoD-12; plan lock 7.
- **Location:** `crates/dedup-engine/src/index.rs:412-416`; `crates/dedup-engine/src/keepset.rs:979-983,1067-1161`
- **Problem:** Backfill is implemented only as a keep-set post-pass. Streaming `DedupIndex` does not merge groups and does not increment `tier1_backfill_candidates`.
- **Impact:** `scan`/`dups` can accept `--tier1-backfill` while producing different grouping and statistics from `keep-set`. The equivalence test does not include this context.
- **Correction:** Make the semantics explicit and consistent across both implementations, or reject the flag on streaming-only commands with a nonzero usage error.
- **Deferrable:** No.

### [P1] F-0076-04 — Tier-1 divergence is suppressed when verification splits

- **Requirement:** DoD-7; spec §3.7 requires “always report, optionally split.”
- **Location:** `crates/dedup-engine/src/keepset.rs:871-905`; `crates/dedup-engine/src/index.rs:270-293`
- **Problem:** Divergence counters are recorded only on the non-split path. With `tier1-verify content|body`, a divergent item is split without incrementing the required body/metadata/recipient counter.
- **Impact:** The principal honesty signal disappears exactly when verification detects the divergence.
- **Correction:** Calculate divergence before applying the optional split decision and record it on both paths.
- **Deferrable:** No.

### [P2] F-0076-05 — Clean empty bodies are treated as unreadable

- **Requirement:** Spec §2.3.2 and DoD-3: absent body differs from genuinely empty clean body.
- **Location:** `crates/pst-dedup-cli/src/scan.rs:821-825,915-919`; `crates/dedup-engine/src/keepset.rs:406-408,488-495`
- **Problem:** `Some("")` is converted to `false`, the same representation used for `None`.
- **Impact:** Sparse messages with a successfully read empty body can be marked degenerate and denied Tier-2 binding.
- **Correction:** Preserve body presence separately from body non-emptiness.
- **Deferrable:** No.

### [P2] F-0076-06 — Tier-2.5 attribution is not fully honest

- **Requirement:** DoD-6c and DoD-7; spec §3.6.
- **Location:** `crates/dedup-engine/src/keepset.rs:1002-1064`
- **Problem:** Attribution includes ineligible/guard-separated items, uses raw display-string equality for BCC-only classification, and runs before the backfill merge.
- **Impact:** `tier2_5_*` counters can describe a pre-merge or guard-induced split as a recipient/body identity split.
- **Correction:** Attribute only eligible Tier-2.5 comparisons, use normalized component values, and compute stats against final grouping semantics.
- **Deferrable:** No.

### [P2] F-0076-07 — `unique-eml` human output omits 0076 grouping statistics

- **Requirement:** Spec §3.10 and the operator-honesty requirement.
- **Location:** `crates/pst-dedup-cli/src/unique_eml_cmd.rs:401-438`
- **Problem:** JSON carries the grouping data, but the human summary prints legacy winner/export counters only.
- **Impact:** Human-only operators cannot see guard, divergence, scope, or backfill evidence.
- **Correction:** Reuse the shared grouping-stat human formatter.
- **Deferrable:** No.

### [P1] F-0076-08 — Required refinement and fixture evidence is absent

- **Requirement:** DoD-11, DoD-12, DoD-15, DoD-18.
- **Location:** `conductor/0076-ContentHashTierHardening/baseline\`; `conductor/0076-ContentHashTierHardening/review.md`
- **Problem:** The baseline contains only `ascii_long_body_digest.txt`; no Aspose/promotions fixture baselines, winner golden, timing results, completed gate results, or final review decision are recorded.
- **Impact:** Split-only behavior and performance claims cannot be independently verified.
- **Correction:** Capture the required baselines, run all option/refinement/equivalence matrices, record timings and gates, then finalize governance state.
- **Deferrable:** No.

## Completeness

Positive evidence includes:

- Character-safe hashing and subject-boundary guards.
- Tier-2 unreadable/degenerate checks in the main grouping path.
- Cross-MID blocking and bind-time provenance.
- Strong body/body-recipient hashing.
- MAPI inline detection.
- Explicit CLI rejection for `body-recip-attach`.
- Additive JSON/CSV fields and identity documentation.
- No ignored tests, `todo!`, `unimplemented!`, or unexplained production stubs found in the audited scope.

The explicit `body-recip-attach` rejection is honest and correctly documented; the earlier “silent no-op” concern does not apply to that path.

## Wiring

- `scan`, `keep-set`, and `unique-pst`: substantially wired.
- `unique-eml`: grouping context reaches execution and JSON, but human stats are incomplete.
- `dups`: flags parse but grouping context is discarded.
- `DedupIndex`: lacks backfill behavior/statistics.
- GUI legacy worker: bypasses the new implementation.
- Documentation is mostly aligned, but governance status and final review evidence are incomplete.

## Verification

Observed:

- Working tree is on `feat/0076-content-hash-tier-hardening` with uncommitted changes.
- Ledgerful status failed with `unable to open database file`.
- No files or Git state were changed by this review.
- No skipped tests or production placeholders were found in the audited scope.

Reported by the orchestrator, not independently executed:

- `dedup-engine` lib: 153 passed.
- CLI lib: 85 passed.
- `keep_set`: 12 passed.
- `unique_pst`: 18 passed.
- Targeted clippy passed.
- Full workspace gate may still be running.

Not independently verified:

- Full fmt/clippy/workspace test gate.
- Ledgerful verify/commit.
- Fixture refinement and winner golden.
- Performance budget.
- Full option-combination equivalence.
- Source-PST SHA evidence and help snapshots.

## Deferred

No new deferred item is proposed.

The existing `D-0076-attach-content` deferral is valid because the CLI rejects the level explicitly and the limitation is documented. It does not excuse the findings above.

## Completion Decision

**FAIL.**

The track cannot be marked complete until the `dups` and GUI paths are wired, backfill semantics are reconciled across grouping implementations, divergence/empty-body honesty is corrected, and the required refinement, performance, verification, and governance evidence is recorded.