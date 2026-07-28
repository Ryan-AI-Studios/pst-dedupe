# 0075 — Keep-Set Winner Policies — Plan

> **Ledger:** `ledgerful ledger start 0075-keepsetwinnerpolicies --category FEATURE --message "Keep-set winner ladder: earliest_date, folder class, source rank, decided_by"`

**Status:** **Ready** — execute after reading `spec.md` (expanded 2026-07-28). **Do not start before 0074 merges** (§5.2).

## Locks (from spec)

1. **Zero silent change** — all new rungs default off; golden regression proves identical winners (§3.9). No always-on demotions, however "obviously correct" (declined on review; ship the stat instead)
2. **Default policy stays `first_seen`** (§2.2.2)
3. **Ladder order** — fidelity → **bcc** → **source_rank** → **folder_class** → policy → path_key → nid; one inversion flag `--rank-folder-class-first` (§3.2)
4. **Never invent a date**; **never fabricate BCC** — absent means absent (§3.3, §3.2.1)
5. **Whole-segment, parent-qualified** folder matching; built-in ladder exact, `--folder-rank` allows segment globs, **no regex** (§3.4)
6. **Documented asymmetry** — `--folder-rank` unmatched=best, `--source-rank` unmatched=worst (§3.5)
7. **Graded fidelity is opt-in**; binary mapping proven exact (§3.6)
8. **Every rung nameable** in `decided_by` (§3.7)
9. **"All Custodians" in CSV *and* JSON**, basename only, cap 8 (§3.7)
10. **CSV append-only**, `keep_set_v1` id retained, JSON additive (§2.2.7–8)
11. **No PST I/O in `dedup-engine`**; classification is a pure string function (§3.1)
12. **Source PSTs read-only** — full-file SHA-256 proof in integration tests

## Phase 0 — Preconditions → DoD foundation

- [ ] Confirm **0074** merged to `main`; rebase branch (0075 shares `scan.rs`, `unique_pst_cmd.rs`, `main.rs`)
- [ ] Confirm 0074's attach `IntegrityReason` variants exist → decides DoD-7 vs **D-0075-graded**
- [ ] `ledgerful ledger start 0075-keepsetwinnerpolicies --category FEATURE --message "…"`
- [ ] `ledgerful scan --impact`; read `.ledgerful/reports/latest-impact.json` (validate `headHash`/`treeClean`)
- [ ] Capture the **golden winner set** from current `main` on `fixtures/aspose_outlook.pst` (this is the pre-change baseline for DoD-10 — capture it *before* any edit)

## Phase 1 — `RankContext` refactor (no behavior change) → DoD-4 foundation

- [ ] Introduce `RankContext { policy, prefer_path, bcc_mode, source_ladder, folder_ladder, folder_class_first, fidelity_mode }` in `dedup-engine::keepset`
- [ ] Migrate `rank_key`, `resolve_groups`, `finalize_with_materialize` and all call sites (CLI ×3, tests)
- [ ] All new fields defaulted inert; **`cargo test -p dedup-engine` must pass unchanged**
- [ ] Golden regression test added here and passing against the Phase-0 baseline

## Phase 2 — Reader capture: date + BCC → DoD-1, DoD-1b

- [ ] `pst-reader`: add `MessageProperties.delivery_time` (`PID_TAG_MESSAGE_DELIVERY_TIME` 0x0E06) and `display_bcc` (`PID_TAG_DISPLAY_BCC` 0x0E02) — both on the PC `read_message_properties` already loads, **no extra I/O**
- [ ] `pst-dedup-cli::scan`: carry `submit_time` + `delivery_time` + `has_bcc` onto `RecoverableScanItem`
- [ ] `KeepPolicy::EarliestDate` + `(has_date, filetime)` key; FILETIME `<= 0` = missing
- [ ] `date_source` resolution + `stats.groups_date_source_mixed`
- [ ] `--prefer-bcc-copy` rung (empty/whitespace BCC = absent) + `stats.winners_without_bcc_peer_had_bcc` computed **regardless of the flag**
- [ ] Unit tests §3.11.1, §3.11.2b (incl. the documented Tier-2 no-op for dates)

## Phase 3 — Folder class + source rank → DoD-2, DoD-3

- [ ] Pure `classify_folder(folder_path) -> (FolderClass, rank)`; whole-segment, case-insensitive, parent-qualified
- [ ] Built-in ladder table (§3.4) behind `--prefer-folder-class` — incl. `sent_items`, `junk_email`, `drafts`, `outbox`
- [ ] Ordered `--folder-rank` custom ladder (replaces built-in, no merge) with **leading/trailing segment globs, no regex**
- [ ] Ordered `--source-rank` over `path_compare_key`, unmatched-worst, ranked **above** folder class
- [ ] `--rank-folder-class-first` inversion (single adjacent-rung swap)
- [ ] `stats.winners_from_recoverable_items` + human-summary hint (signal only — winners must not move)
- [ ] Unit tests §3.11.2–5 (user folder named `Purges` **not** demoted; INC `-2` flip; CEO-archive-beats-junior-inbox; inversion flag moves exactly that pair)

## Phase 4 — Explainability + duplicate provenance → DoD-5, DoD-6, DoD-6b

- [ ] `decided_by` computed from rank-tuple comparison (winner: rung that beat runner-up; dup: rung that lost)
- [ ] Append decision CSV columns in spec order (incl. `has_bcc`); header-prefix test
- [ ] `KeepEntry`: `folder_class`, `decided_by`, `duplicate_source_count`, `duplicate_sources` (cap 8), `duplicate_sources_truncated`
- [ ] **Same aggregate on decision-CSV unique rows and on `export_messages.csv`** — pipe-delimited, **basename only**, following D-0073-basename mode if it ships
- [ ] Free-text columns routed through the 0073 CSV-injection-safe writer
- [ ] Unit tests §3.11.6, §3.11.8, §3.11.8b (three-surface value equality)

## Phase 5 — Graded fidelity (opt-in) → DoD-7

- [ ] `--fidelity-rank binary|graded`; graded tier table (§3.6); item rank = worst tier present
- [ ] Exhaustive reason→tier mapping test (unmapped new reasons default to tier 3)
- [ ] Binary-mode equivalence test (`{0}→0`, `{1..4}→1`)
- [ ] If 0074 reasons absent: **stop**, record **D-0075-graded** in `review.md`, keep binary only

## Phase 6 — CLI + Desk surface → DoD-8, DoD-9

- [ ] Flags on `keep-set`, `unique-eml`, `unique-pst` with identical names/help
- [ ] Update **both** duplicated policy parsers (`main.rs`, `unique_pst_cmd.rs`) and their error messages
- [ ] Desk wizard: `earliest_date` in the policy `ComboBox` + `Prefer folder class` and `Prefer BCC copy` checkboxes; arg-mapping unit test
- [ ] `cargo check -p pst-dedup-gui`

## Phase 7 — Compatibility, determinism, integration → DoD-10, DoD-11, DoD-12, DoD-14

- [ ] Golden winner set unchanged with default flags (from Phase 1)
- [ ] Pre-0075 `keep_set_v1` JSON deserialize test
- [ ] Shuffled-input determinism test
- [ ] Integration: temp-dir `a.pst` / `a-2.pst` copies; `--source-rank` flips winner; **full-file SHA-256 unchanged both files**
- [ ] `--help` / header snapshots

## Phase 8 — Docs → DoD-13

- [ ] `docs/unique-pst-export.md` "Winner policies": ladder diagram, policy table, folder-class ladder + its judgments, asymmetry table, glob syntax
- [ ] **Sender-copy guidance:** why BCC lives only on the sender's copy, and to enable `--prefer-bcc-copy` with `--prefer-folder-class` together
- [ ] **`Recoverable Items/Versions` warning:** copy-on-write items may be structurally altered (subject/body/attachments/participants/dates) yet still tie on `submit_time` — the folder ladder, not `earliest_date`, is what separates them
- [ ] **`first_seen` = sorted input path order** with the INC `-2` example + `--source-rank` remedy
- [ ] Closed vocabularies (`decided_by`, `folder_class`, `date_source`) for downstream parsers
- [ ] Cross-link **0080** QC sampling by class and **0081** runbook

## Phase 9 — Gate + finalize → DoD-15, DoD-16

- [ ] Targeted tests, then full gate: `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo test --workspace`
- [ ] `ledgerful impact`; `ledgerful verify`
- [ ] Purge anything written under `output\`
- [ ] `review.md` + **D-0075-*** rows in `docs/deferred.md` (`scope`, `gui`, `storeids`, `locale`, and `graded`/`exportcsv` if deferred)
- [ ] `conductor.md` + `sequencing.md` → **Completed**
- [ ] `ledgerful ledger commit <tx-id> --summary "…" --reason "…"`

## Suggested order

1. Golden baseline **first** (it is worthless captured after an edit)
2. `RankContext` refactor with zero behavior change
3. Reader capture + `earliest_date` + BCC rung (one reader touch, two rungs)
4. Source rank → folder class (the actual INC fix + custodian priority)
5. `decided_by` + duplicate provenance + honesty stats
6. Graded fidelity (skippable)
7. CLI/Desk → compatibility tests → docs → gate

## Handoff notes

- **Do not** change default winners; a golden-test diff without a flag is a bug — including for `Recoverable Items/Versions`.
- **Do not** use raw substring matching in the built-in ladder, or accept regex in `--folder-rank`.
- **Do not** invent dates from mtime / `PidTagLastModificationTime` / wall clock, or infer BCC from anything.
- **Do not** ship the "All Custodians" aggregate to JSON only, or put absolute client paths in it.
- **Do not** reorder or reinterpret existing CSV columns; append only.
- **Do not** widen grouping semantics (custodial scope → D-0075-scope; tier changes → 0076).
- **Do not** start before 0074 merges.
- **Rollback:** unregister the CLI flags — engine additions go inert and output returns to pre-0075.
