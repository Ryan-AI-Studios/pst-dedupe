# opencode-review — 0089 UniqueEmlAttachLedger (spec/plan review, review only)

- **Series context / verdict summary:** see `../../opencode-review.md` — verdict: **Right goal, wrong crate scope**.
- **Method:** code snapshot claims verified against `main` @ `c5437d0`; no code edits made.

**Verified:** `AttachLedgerSink` / `AttachLedgerRow` exist in `crates/pst-dedup-cli/src/unique_export_report.rs:649,907`; unique-pst flags default `full` (`crates/pst-dedup-cli/src/unique_pst_cmd.rs:169-173`); `UniqueEmlCliArgs` has `attach_parts_failed` counter only (`crates/pst-dedup-cli/src/unique_eml_cmd.rs:108,318`). Spec snapshot is accurate.

**Strengths:** reuse-don't-reinvent lock on the 0073 row schema; explicit Mode A / exit-64 consistency requirement; row-cap parity; right operator pain.

**Findings / blind spots:**

1. **Cross-crate impact is missing — this is not a CLI-only wire.** On unique-pst, ledger rows come from two feeds: (a) `pst-writer`'s `AttachEventSink` events during materialize (`crates/pst-writer/src/production.rs:814`, sink impl at `unique_export_report.rs:1192`), and (b) pre-collected `soft_skip_attach_records` + `mark_promoted_winner` (`unique_pst_cmd.rs:2127-2169`). The unique-eml path has **neither**: `eml_pack` soft-fails surface only as a counter and a `tracing` log (`crates/dedup-engine/src/eml_pack.rs:828-834`), with **no structured per-attach detail** (locus, reason, filename, attach NID). Producing ledger rows that satisfy DoD-2 therefore requires a *new event/callback surface in `dedup-engine/src/eml_pack.rs`*. Consequences to fix in spec/plan before start:
   - ledger category/start should cover both crates (plan says `ledgerful ledger start crates/pst-dedup-cli`);
   - §8 verification must add `cargo test -p dedup-engine` and widen clippy to include it.
2. **Two failure-reporting mechanisms risk a second dialect.** Spec lock 1 says reuse the 0073 row schema — good — but the *reason taxonomy* mapping is unstated: `EmlWriteError` variants ≠ pst-writer attach event reasons. Phase 0 should lock an explicit mapping table (EML-path failure reason → 0073 `reason_code`), including a rule for unmapped reasons (fail to a generic code, never silently drop the row).
3. **Mode A parity:** unique-eml accepts `promote_on_attach_fail` (0083). With ledger on, promoted winners need `winner_promoted` rows like unique-pst. Plan Phase 0 last bullet covers this — keep it as a hard requirement, not a confirmation.
4. **Report-dir convention:** unique-eml writes a volume pack under `--out`; the CSV should land at pack root exactly like unique-pst. Plan Phase 0 covers; suggest locking it in the spec §3 since operators script against these paths (0081's `--ledger-path-mode` exists precisely for that).
5. **Minor:** DoD-4's "ledger on or off" exit behavior — add one regression where ledger init *fails* mid-mode-`full`: unique-pst treats that as a report-pack error, fail closed (`unique_pst_cmd.rs:2104-2124`). The eml path must match that honesty or the parity claim has a hole.

**Opportunities:** if eml_pack gains an attach-failure event, consider making it the *single* structured failure surface for both export paths long-term (pst-writer's `AttachEventSink` and eml_pack's event share a reason vocabulary in dedup-engine), which would also pay off for 0091.
