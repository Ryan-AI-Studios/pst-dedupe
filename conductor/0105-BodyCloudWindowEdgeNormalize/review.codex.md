# Track Completion Audit — 0105-BodyCloudWindowEdgeNormalize

## Verdict: PASS

## Scope Reviewed

Reviewed all of `spec.md` and `plan.md`, the working-tree implementation and documentation diff against `origin/main`, downstream CLI/report wiring, relevant tests, and completeness markers.

The workspace is read-only, so `review.codex.md` could not be persisted here; this is the complete audit content.

## Requirement and DoD Matrix

| Requirement | Status | Evidence | Tests | Gap |
|---|---|---|---|---|
| Edge candidates normalize before classification | Met | `body_cloud_links.rs:258-263` | Punctuated duplicate test | None |
| Only classified URLs enter `seen` | Met | Insert is inside `classify_url` `Some` arm at `:263-272`; empty candidates return at `:260-262` | Edge tests | None |
| Over-length identity inserted by `note_overlength` | Met | `:104-112`; all relevant callers use this choke point | Over-length and nested-tail tests | None |
| Max-links checked before over-length insertion | Met | `:396-402` | `max_links_plus_overlength_sets_both_flags_and_prefix` | None |
| Unique unseen edge remains WINDOW | Met | `:264-272` | `body_window_bare_url_cut_at_boundary_not_kept` | None |
| Required regression tests | Met | New tests at `:1098-1152` reconstruct `url + "."` and the same over-length URL | Reported: 8 `body_window`, 43 `body_cloud` tests pass | None |
| Documentation and changelog | Met | `docs/unique-pst-export.md:588`, `CHANGELOG.md:10-12` | N/A | Deferred-row status still awaits Phase 4 bookkeeping |
| Scope boundaries preserved | Met | Only `dedup-engine` scanner, tests, docs, and governance references changed | Diff inspection | None |
| DoD-4 recording | Pending process step | Registry remains Ready/In progress; canonical `review.md` and ledger commit are absent | Explicitly exempted by handoff | Orchestrator must finish Phase 4 |

## Findings

No supported P0, P1, P2, or P3 findings.

## Completeness Sweep

- No new TODO, FIXME, stub, placeholder, `unimplemented!`, or production `unwrap`/`expect` paths were introduced.
- `expect` occurrences are test-only.
- No silent insertion of empty or unclassified URLs exists in the edge path.
- No cap, C+A flag, CSV schema, frontend, PST reader/writer, or GUI changes were introduced.
- No migration, generated contract, dependency, security, signing, or provenance boundary was affected by the product change.
- `git diff --check` passed for the scoped files.

## Wiring and Regression Review

Production flow is complete:

`scan_body_cloud_links` → `collect_bare` / HTML bare collection → `handle_window_edge_bare` → normalized classification and `seen` dedupe → `BodyCloudScan` flags → CLI preparation → existing `BODY_CLOUD_LINK_WINDOW` report mapping.

The punctuated fixture is correctly aligned at the 100,000-character boundary:

- kept URL length: 52
- edge head length: 47
- trailing tail: `x?d=1.`
- padding: 99,900 characters
- reconstructed value: `url + "."`

The over-length fixture reconstructs the exact classified URL and verifies that the first over-length occurrence suppresses a later edge WINDOW marker.

Against the old implementation:

- The punctuated test would classify `url + "."`, miss the kept normalized URL in `seen`, and set `window_dropped`.
- The over-length test would not find the earlier URL in `seen`, and would also set `window_dropped`.

Both tests therefore encode the intended regression and would fail on the old behavior.

## Verification Evidence

Observed during review:

- `git diff --check`: passed.
- Scoped implementation diff contains only the intended scanner, tests, documentation, and governance changes.
- Fixture arithmetic and boundary placement verified statically.
- `ledgerful ledger status --compact`: unavailable; failed with `rusqlite_migration … unable to open database file`.
- `ledgerful scan --impact`: could not complete cleanly in read-only mode; report writing was unavailable. Cached impact report identified the dirty tree and stale test mapping.

Reported by the orchestrator, not independently rerun:

- `cargo test -p dedup-engine body_window`: 8 passed.
- `cargo test -p dedup-engine --lib body_cloud`: 43 passed.
- `cargo fmt --all --check`: passed.
- `cargo clippy -p dedup-engine --all-targets -- -D warnings`: passed.

Workspace clippy/test and `ledgerful verify` remain pending publish, as stated in the handoff.

## Deferred Candidates

None. No difficult non-blocking P3 issue was identified.

The remaining `docs/deferred.md` closure, registry status, canonical `review.md`, ledger commit, workspace gates, and `ledgerful verify` are Phase 4 orchestrator tasks and are not product defects.

## Completion Decision

Engineering requirements DoD-1 through DoD-3 are satisfied. The implementation is correctly wired, scoped, regression-tested, and documented.

The orchestrator may complete Phase 4 bookkeeping and publish verification.