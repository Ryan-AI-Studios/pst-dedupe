# Track Completion Review — 0096-PermissionTypeExtract

## Verdict: PASS

## Scope

Four-crate extract of `PidNameAttachmentPermissionType` (PtypInteger32 MAY-if-present):

`pst-reader` → `AttachmentInfo` → `pst_materializer` → `CanonicalAttachment` → `from_canonical_message{,_owned}` → cloud-pointer writer → live-read QC.

Closes **D-0092-permission-type-extract**.

Branch: `track/0096-PermissionTypeExtract` (from `e0702bc`).

## Reviewers / rounds

| Round | Reviewer | Result |
|---|---|---|
| Internal | Orchestrator + implementer gates | Clean after wiring |
| Codex r1 | gpt-5.6-luna high | **FAIL** — P1 QC cloud scope; P2 weak DoD-1/hash tests |
| Codex r2 | gpt-5.6-luna high | **FAIL** — P2 materializer bridge / hash discrimination / non-cloud NPMAP |
| Codex r3 | gpt-5.6-luna high | **FAIL** — P2 materializer→owned-writer e2e still missing |
| Codex r4 | gpt-5.6-luna high | **PASS** — no findings |

Raw: `review.codex.r1.md` … `review.codex.r4.md`.

## DoD matrix

| DoD | Status | Evidence |
|---|---|---|
| DoD-1 | Met | `from_canonical_cloud_permission_type_round_trips`; `materializer_owned_writer_preserves_permission_type`; QC preserve/missing live-read |
| DoD-2 | Met | Non-cloud empty NPMAP plan; hasher `filename:size` only; permission-only hash projection + size control |
| DoD-3 | Met | Fidelity `PidNameAttachmentPermissionType` Preserved; `D-0092-permission-type-extract` closed |
| DoD-4 | Met | `cargo fmt --all --check`; clippy four crates `-D warnings`; focused reader/engine/writer/cli QC suites |
| DoD-5 | Met | This `review.md`; conductor **Completed**; ledger FEATURE commit |

## Key locks honored

- Open-world i32; no invent; extract always / write cloud-pointer only
- QC gated by `should_compare_permission_type(is_cloud_link, src_perm)`
- `AttachDetail` struct (not 7-tuple); digest preimage excludes permission
- Hasher isolation (lock 7)

## Deferred

None from Codex r4 (no residual lows).

## Operator note

INC* attach-table cloud rows remain 0 — synthetic fixtures only exercise this path.
