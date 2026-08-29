# Track Completion Audit — 0106-UniqueEmlNestedMime

## Verdict: FAIL

## Scope Reviewed

Reviewed `658d272` against `origin/main` at `40d5a43`, including `spec.md`, `plan.md`, implementation diff, tests, docs, registry, deferred ledger, and worktree state.

DoD-4 artifacts are now present:

- [`review.md`](/C:/dev/Dedupe/conductor/0106-UniqueEmlNestedMime/review.md)
- Registry row is **Completed** at [`conductor.md:260`](/C:/dev/Dedupe/conductor/conductor.md:260)
- Ledger transactions are reported as committed, but Ledgerful could not be independently queried under read-only restrictions.

The worktree also contains unrelated unstaged/untracked changes; none overlap the product diff.

## Requirement and DoD Matrix

| Requirement | Status | Evidence |
|---|---|---|
| Method-5 DTO reconstructs RFC 5322 MIME | Met | [`eml_pack.rs:982`](/C:/dev/Dedupe/crates/dedup-engine/src/eml_pack.rs:982) |
| Method-5 missing-DTO/depth skips precede stream open | Met | [`eml_pack.rs:982-1003`](/C:/dev/Dedupe/crates/dedup-engine/src/eml_pack.rs:982) |
| Dedicated depth/unparsed ledger reasons | Met | [`eml_pack.rs:901-924`](/C:/dev/Dedupe/crates/dedup-engine/src/eml_pack.rs:901) |
| Method-1 `message/rfc822` remains raw 8bit dump | Met | [`eml_pack.rs:1003-1022`](/C:/dev/Dedupe/crates/dedup-engine/src/eml_pack.rs:1003) |
| Recursive counters and parent-depth semantics | Met | [`eml_pack.rs:1093-1113`](/C:/dev/Dedupe/crates/dedup-engine/src/eml_pack.rs:1093) |
| Nested source-NID routing | Partial | Valid NIDs are used, but missing NIDs become `0` |
| Inner headers and boundary isolation | Met | [`eml_pack.rs:705-742`](/C:/dev/Dedupe/crates/dedup-engine/src/eml_pack.rs:705), [`eml_pack.rs:1294`](/C:/dev/Dedupe/crates/dedup-engine/src/eml_pack.rs:1294) |
| CLI flag, extraction, effective depth, summary | Met | [`main.rs:478-485`](/C:/dev/Dedupe/crates/pst-dedup-cli/src/main.rs:478), [`unique_eml_cmd.rs:270-385`](/C:/dev/Dedupe/crates/pst-dedup-cli/src/unique_eml_cmd.rs:270) |
| Nested incomplete attachment lists remain honest | Unmet | `attachments_incomplete` is discarded |
| Required unit/integration tests present | Met by source audit | Nested DTO, no-DTO, method-1, depth, ceiling, and clap tests exist |
| Documentation and D-0067 narrowing | Met | Docs and [`docs/deferred.md:705`](/C:/dev/Dedupe/docs/deferred.md:705) |
| DoD-4 recorded completion | Met, ledger state unverified | Review artifact and Completed registry row exist |

## Findings

### [P1] Nested incomplete attachment lists are silently omitted

Confidence: High

Requirement: DoD-1; spec §3.1/§3.12; product lock “no silent attach/count drops.”

Location: [`embedded.rs:464`](/C:/dev/Dedupe/crates/pst-reader/src/messaging/embedded.rs:464), [`pst_materializer.rs:912`](/C:/dev/Dedupe/crates/pst-dedup-cli/src/pst_materializer.rs:912), [`eml_pack.rs:1028-1060`](/C:/dev/Dedupe/crates/dedup-engine/src/eml_pack.rs:1028)

Problem: The reader sets `NestedCanonicalMessage.attachments_incomplete` when child attachment rows are omitted. The materializer preserves it, but `nested_to_canonical` drops the field. The EML writer then emits the nested message with no failure event, failed count, or degraded marker.

Evidence: `attachments_incomplete` is populated in the reader and preserved in the DTO, but has no use anywhere in `eml_pack.rs`. The existing PST writer explicitly emits a synthetic metadata-failure event for this condition at [`production.rs:3874`](/C:/dev/Dedupe/crates/pst-writer/src/production.rs:3874).

Failure scenario: A nested attachment table contains one unreadable child row. The reconstructed EML contains the readable children but reports the nested message as fully reconstructed, with no `export_attachments.csv` indication that a child was omitted.

Correction: Carry nested attachment-list incompleteness into the recursive EML result and emit an appropriate inner-subject attachment failure event/counter, without inventing a MIME part.

Verification: Add a synthetic `NestedCanonicalMessage { attachments_incomplete: true }` test asserting a failure count/event and degraded export state.

Deferrable: No

### [P2] Missing `source_msg_nid` is converted to invalid NID `0`

Confidence: High

Requirement: Spec §3.8 and DoD-1.

Location: [`eml_pack.rs:1037`](/C:/dev/Dedupe/crates/dedup-engine/src/eml_pack.rs:1037)

Problem: `nested.source_msg_nid.unwrap_or(0)` turns an absent source identity into a valid-looking parent locus. Child attachments can then use `(source_path, 0)` for stream lookup; in-memory child data bypasses stream validation entirely.

Failure scenario: A public caller supplies a nested DTO with `source_msg_nid: None` and a child attachment. The child may be written or a stream may be requested against NID `0`, instead of being explicitly soft-failed as required.

Correction: Preserve the optional source NID and make child attachment handling soft-fail when it is absent; never substitute `0` as a stream parent.

Verification: Add a nested DTO test with `source_msg_nid: None` and a child attachment, asserting no child part is emitted and a soft-failure event is recorded.

Deferrable: No

## Completeness Sweep

No P0 findings were found. The committed diff contains no new production `unwrap`, `expect`, panic, TODO, stub, fake RFC822 body, schema-ID bump, GUI change, unique-PST rewrite, or D-0067 closure.

The required method-5 honesty behavior is present: missing DTOs do not dump MAPI bytes as `message/rfc822`.

## Wiring and Regression Review

The primary path is correctly connected:

`unique-eml CLI → winner re-materialization → nested extraction → recursive RFC 5322 writer → attach ledger/summary`

Method-1 RFC822 attachments remain on the raw 8bit dump path. `parents_only` still omits attachments. Depth handling uses the shared effective value and correctly writes exactly the permitted number of nested levels.

The two findings above affect nested child-fidelity reporting and source-locus safety.

## Verification Evidence

Observed now:

- `cargo fmt --all --check`: passed.
- `git diff --check`: passed.
- Existing target binary exposes `--max-embedded-depth`, default 3, valid 1–8.
- Existing target binary rejects `0`, `9`, and `abc` with the expected usage error.
- Required track tests are present in source.

Reported by orchestrator/canonical review:

- `eml_pack` tests: 34 passed.
- `unique_eml` tests: 13 passed.
- `unique_eml_depth` tests: 4 passed.
- Workspace tests and clippy previously passed.

Not independently verifiable now:

- Current clippy/test execution is blocked by access denied on `C:\dev\Dedupe\target\debug\.cargo-lock`.
- `ledgerful verify` and ledger status are blocked by Ledgerful database access under read-only restrictions.
- AI-Brains context is unavailable because `AI_BRAINS_KEY` is unset.

## Deferred Candidates

`D-0067-embedded-depth` remains correctly open for matter/Relativity child-document extraction, the 32 MiB budget, and the hard cap of 8.

No P3-only deferral is appropriate. The two findings are P1/P2 and must be fixed.

## Completion Decision

DoD-4 is now satisfied, resolving the prior review’s completion-record finding. However, the implementation still has one production-reachable silent child-attachment omission and one invalid missing-NID fallback. The track therefore remains **FAIL** pending fixes and re-review.