# 0133 — ProcessUnaccountedLeaves — Review

## Scope

Process names unextracted PST inventory leaves and offers **Extract remaining** that queues those ids only. `unaccounted_for` arithmetic is frozen (0126). Poller completes accepted jobs that reach terminal before the first `running` snapshot.

## DoD matrix

| Item | Result | Evidence |
|---|---|---|
| DoD-1 Preserve arithmetic; name leaves | PASS | `unaccounted_for` body unchanged. Host emits `unextracted_psts` + `failed_unlogged`. UI lists basenames + failed-unlogged footnote. |
| DoD-2 Extract remaining queues the gap and drains | PASS | Work from `pg.unextracted_psts`; `extract_all_should_start` before queue write; one production `extract_queue.set(Vec::new())`. `poll_finished_ok` + `accepted_job` drain. |
| DoD-3 Fast ingest refreshes sources | PASS | `should_reload_stale_importing` plus accepted-job terminal reload. |
| DoD-4 Recorded | PASS | This file; registry Completed; ledger FEATURE `bca7115d-a7aa-4b83-943e-258d1e54530a` on the product squash. |

## Gates

| Command | Result |
|---|---|
| `cargo fmt --all --check` | pass |
| `cargo clippy --workspace --all-targets -- -D warnings` | pass |
| `cargo test -p dedupe-chrome --lib` | 129 passed |
| `cargo test --manifest-path crates\dedupe-chrome\ui\Cargo.toml` | 54 passed |
| `cargo test --workspace` | pass |
| `ledgerful verify` | pass |
| CI (PR **#150**) | fmt, clippy, test (13m16s), audit, deny, chrome-ui, verify-parity **green**. Bugbot NEUTRAL (does not block). |
| Final cross-model gate | **PASS**, 0 findings (`conductor/0133-ProcessUnaccountedLeaves/review.codex-r2.md`) |

## Reviewer rounds

1. Internal: FAIL — drop unlistens + missing label tests; fixed.
2. Internal re-review: PASS.
3. Codex round 1: **FAIL** — P1 fast terminal drain; P2 job Resume / drop names / silent listener.
4. Fixes: `poll_finished_ok` + `accepted_job`; failed/paused job Resume; unqueued names on every drop error; `attach_drop_listener`.
5. Internal: PASS ([review](0131be24-1130-49dd-adce-76f5d85a09af)).
6. Codex round 2 (`review.codex-r2.md`): **PASS**, 0 findings. Final gate. Cross-model fallback: composer-2.5-fast (gpt-5.6-sol unavailable in this session).

## HITL (owner)

Owner chrome EXE: two-PST Extract remaining + drop. Spec allows after merge. Codesign is **D-0062-codesign**. INC* unique-pst is not a gate.

## Residual lows (deferred)

| ID | Item |
|---|---|
| (this track) | none above low |
| Owner EXE smoke | optional after merge; not an engineering block |

## Publish

- Branch: `track/0133-0137-series-v`
- PR: **#150** https://github.com/Ryan-AI-Studios/pst-dedupe/pull/150
- Merge SHA: `a8287b43988cc990726a6fd48d3738d3b074767c` (short `a8287b4`)
- Commit: `track(0133-0137): chrome-mockup operational parity (#150)`
- Ledger FEATURE tx `bca7115d-a7aa-4b83-943e-258d1e54530a` COMMITTED on the product squash
- Locks held: schema 41; `unaccounted_for` formula frozen; 0122 Busy keep-queue; no BCC-default
