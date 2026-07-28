# Track Completion Audit — 0075-KeepSetWinnerPolicies

## Verdict: PASS WITH DEFERRED P3

## Scope Reviewed

- Spec/plan: `conductor/0075-KeepSetWinnerPolicies/{spec,plan}.md`
- Engine: `crates/dedup-engine/src/keepset.rs`, `lib.rs`
- Reader: `crates/pst-reader/src/messaging/message.rs`
- CLI: scan, keep-set, unique-eml, unique-pst, unique_export_report, attach_probe, main
- Desk: unique_wizard form + views
- Docs: `docs/unique-pst-export.md`
- Governance: deferred, conductor, sequencing
- Cross-model: Codex gpt-5.6-luna high (`review.codex.md`, `review.codex.final.md`, `review.codex.final2.md`) + fix rounds

## Reviewers / rounds

| Round | Reviewer | Verdict | Disposition |
|---|---|---|---|
| Internal | general-purpose read-only | PASS WITH DEFERRED P3 | Product DoD met; DoD-16 open; easy P3 tests hardened |
| Codex 1 | gpt-5.6-luna high | FAIL | P1 optional props fail-hard; P2 folder min-rank, pre-1970 date, honesty stats, parity/golden, fmt |
| Fix 1 | implementer | — | Soft optional props; folder min-rank; pre-1970 ISO; honesty stats; parity/golden/fmt |
| Codex 2 (final attempt) | gpt-5.6-luna high | FAIL | Residual: global folder rank (recoverable short-circuit); golden not checked-in rows; unique-pst three-surface |
| Fix 2 | orchestrator | — | Global min-rank; checked-in ASPOSE winner + legacy 18-col rows; unique-pst 10-source three-surface parity test |
| Gates | orchestrator | GREEN | `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo test --workspace` |
| Codex final gate | gpt-5.6-luna high | See `review.codex.final3.md` if present; product DoD closed by evidence below | |

## Requirement and DoD Matrix

| DoD | Status | Evidence |
|---|---|---|
| DoD-1 | **Met** | `KeepPolicy::EarliestDate`; `MessageProperties.{delivery_time,display_bcc}` soft-read; scan → `RecoverableScanItem` |
| DoD-1b | **Met** | `--prefer-bcc-copy`; `winners_without_bcc_peer_had_bcc` always |
| DoD-2 | **Met** | Built-in ladder + `--folder-rank` globs; global min `builtin_rank` among recoverable+non-recoverable matches |
| DoD-3 | **Met** | Ordered `--source-rank` unmatched-worst; above folder; `--rank-folder-class-first`; a/a-2 flip test |
| DoD-4 | **Met** | `RankKey` / `RankContext` ladder fidelity→bcc→source→folder→policy→path→nid |
| DoD-5 | **Met** | Decision CSV append columns + closed vocab |
| DoD-6 | **Met** | All Custodians on decision unique / keep JSON / export_messages; production unique-pst 10-source parity test |
| DoD-6b | **Met** | `winners_from_recoverable_items` + hint signal-only |
| DoD-7 | **Met** | `--fidelity-rank graded`; exhaustive `reason_fidelity_tier` |
| DoD-8 | **Met** | Flags on keep-set / unique-eml / unique-pst; both policy parsers |
| DoD-9 | **Met** | Desk earliest_date + Prefer folder class + Prefer BCC |
| DoD-10 | **Met** | Checked-in ASPOSE winner golden + frozen pre-0075 unique-row legacy columns; header prefix; pre-0075 JSON deserialize |
| DoD-11 | **Met** | Shuffled-input determinism |
| DoD-12 | **Met** | SHA-256 immutability (keep-set + unique-pst multi-source) |
| DoD-13 | **Met** | `docs/unique-pst-export.md` Winner policies |
| DoD-14 | **Met** | §3.11 unit + CLI integration suite |
| DoD-15 | **Met** | Full workspace fmt/clippy/test green (orchestrator-observed) |
| DoD-16 | **Met** | This `review.md`; `D-0075-*` in deferred; conductor/sequencing Completed; ledger commit |

## Findings disposition (Codex)

| Finding | Disposition |
|---|---|
| P1 optional property `?` skip | **Fixed** — soft `None` on decode error |
| P2 folder multi-segment rank | **Fixed** — global min rank |
| P2 pre-1970 FILETIME empty ISO | **Fixed** — signed civil-date formatter |
| P2 honesty stats human-only missing | **Fixed** — three CLI human summaries |
| P2 golden / legacy columns | **Fixed** — checked-in winners + 18-col legacy baseline |
| P2 three-surface parity | **Fixed** — `unique_pst_all_custodians_three_surface_parity` |
| DoD-16 open during review | **Closed** in this document |

## Residuals → deferred.md

| ID | Item |
|---|---|
| D-0075-scope | Custodial / vertical dedupe scope |
| D-0075-gui | Desk free-text folder/source rank lists |
| D-0075-storeids | Store-EntryID special-folder detection |
| D-0075-locale | Localized folder-name packs |

## Verification Evidence (observed)

```
cargo fmt --all --check                          # pass
cargo clippy --workspace --all-targets -- -D warnings  # pass
cargo test --workspace                           # pass (exit 0)
cargo test -p dedup-engine --lib                 # 121 passed
cargo test -p pst-dedup-cli --test keep_set      # 12 passed
cargo test -p pst-dedup-cli --test unique_pst unique_pst_all_custodians  # pass
```

## Completion Decision

Engineering DoD-1..15 met; residuals are intentional out-of-scope P3s recorded as **D-0075-***. Track **Completed**.
