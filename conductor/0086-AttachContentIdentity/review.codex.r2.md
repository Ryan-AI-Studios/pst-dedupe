# Track Completion Audit — 0086-AttachContentIdentity

## Verdict: FAIL

## Scope Reviewed

Read-only review of the uncommitted working tree against `origin/main`, including track `spec.md`, `plan.md`, prior reviews, all changed implementation/tests/docs, and untracked files.

## Requirement and DoD Matrix

| DoD | Status | Evidence / gap |
|---|---|---|
| DoD-1 CLI level live | Met | All five surfaces expose and parse `body-recip-attach`; warning path exists. |
| DoD-2 Streaming digests | Partial | Streaming helper is correct, but partial attachment enumeration can omit slots. |
| DoD-3 Grouping effect | Met | Synthetic PST test proves same name/size, different bytes split. |
| DoD-4 Choice B honesty | Unmet | Parser-level enumeration failures can still become partial/empty attachment lists without sentinels or skips. |
| DoD-4b NIST KAT | Met | Multi-block 1,000,000-`a` vector present. |
| DoD-5 Refinement | Met | Hasher tests prove attach identity subdivides `body-recip`. |
| DoD-6 Default safe | Met by inspection | Full-stream hashing is gated by `includes_attach_content()`. |
| DoD-7 Docs/deferred | Met | Product docs are live/honest; D-0076 is closed and D-0086 residuals are recorded. |
| DoD-8 Gates | Not independently verifiable | Cached gates passed before the latest `scan.rs` changes; no rerun was performed due read-only scope. |
| DoD-9 Governance | Deferred/process | `review.md`, board completion, and ledger commit remain orchestrator work as stated. |

## Prior-Finding Reverification

- **P1 enum BestEffort hijack — fixed for returned errors.** `classify_attach_enum_for_identity(true, …)` always returns `Skip` at `crates/dedup-engine/src/integrity.rs:769-782`, with unit coverage. The scan call site uses it at `crates/pst-dedup-cli/src/scan.rs:799-861`.

- **P2 truncation stats — fixed in the scan path.** State tracking exists in `attach_content_hash.rs:68-80`; the index records truncation at `scan.rs:1332-1335`; human output is rendered at `grouping_cli.rs:185-189`; deep-attach rebuild preservation is present at `scan.rs:1495-1503`.

- **P2 stale documentation — fixed.** `docs/unique-pst-export.md:390-432` and the runbook describe `body-recip-attach` as live. The old claim appears only in the historical prior review artifact.

- **Governance DoD-8/9 — intentionally still orchestrator-owned.** The board remains In Progress at `conductor/conductor.md:199-201`, consistent with the supplied handoff.

## Findings

### [P1] Partial attachment enumeration still violates fail-closed Choice B

Confidence: High

Requirement: DoD-2, DoD-4; Choice B must produce one real digest or unread sentinel for every identity-relevant attachment.

Location: `crates/pst-reader/src/messaging/attachment.rs:325-397`; `crates/pst-dedup-cli/src/scan.rs:794-799`.

Problem: `list_attachments_inner` silently ignores attachment rows whose property context fails to parse:

```rust
if let Ok(pc) = PropContext::load(att_data) {
    ...
    attachments.push(...)
}
```

The function can therefore return `Ok` with a partial or empty list. The scan’s fail-closed classifier only runs when `list_attachments` returns `Err`.

Failure scenario: A message has an attachment subnode whose property context is corrupt. That row is dropped, no sentinel is generated, and the remaining attachment set—or zero slots—feeds the strong hash. An attachment-bearing message can consequently match a no-attachment or incomplete peer.

A second related bypass is `props.has_attachments.unwrap_or(false)` at `scan.rs:794-795`; when that property is absent, identity enumeration is skipped entirely.

Correction: Make attachment enumeration report partial failure, or return an error on any identity-relevant row that cannot be parsed. Under `body-recip-attach`, skip the message rather than hashing a partial list. Avoid treating missing attachment-presence metadata as proof of zero attachments.

Verification: No test covers malformed or partial attachment-table enumeration.

Deferrable: No.

## Completeness Sweep

No new production placeholders, fake digests, `read_to_end`, multi-GB attachment buffers, or production `unwrap`/`expect` paths were found in the 0086 implementation. Choice B sentinels, sorting, cloud handling, empty-file handling, and lower-level default gating remain intact.

## Verification Evidence

- `git diff --check origin/main -- . ':!.ledgerful'`: passed.
- Cached `latest-verify.json`: passed, but timestamped before the latest `scan.rs` modification; not current-tree evidence.
- `ledgerful ledger status --compact`: failed with `unable to open database file`.
- Cargo and Ledgerful verification were not rerun because this review was explicitly read-only.

## Deferred Candidates

The intentional residuals remain valid:

- `D-0086-embedded-email-hash`
- `D-0086-digest-probe-unify`
- Process DoD-9 closeout

## Completion Decision

The prior listed fixes are present, but partial attachment enumeration still permits omitted identity slots. Engineering completion is not met. Fix the P1, add a malformed-enumeration regression test, rerun current-tree gates, and then perform governance closeout.