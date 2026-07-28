# 0077 — CRC Noise Control & Export Risk — Plan

> **Ledger:** `ledgerful ledger start 0077-crcnoiseandexportrisk --category FEATURE --message "CRC noise control: data-path integrity counters, bounded emission, export_risk on the existing vocabulary"`

**Status:** Ready.

## Locks (from spec)

1. **Count first, log second** — every suppressed line is still counted exactly (§2.3.1)
2. **Counters in the data path, not the log path** — nothing may depend on a subscriber being installed (§2.3.2, D3)
3. **One risk vocabulary** — reuse `PreflightRecommendation`; no `low|elevated|high` (§2.3.3)
4. **CRC stays warning-only and non-fatal**; `crc_skip_rate` keeps its exact meaning (§2.3.4, §3.4)
5. **Bounded memory under corruption** — every accumulator gets a cap + an exactness flag (§2.3.5)
6. **Sources read-only**; ScanPST runs on a copy because Microsoft documents that it mutates (§2.3.6)
7. **New lines carry counters, never content** (§2.3.7)
8. **Additive JSON / append-only CSV / `#[serde(default)]`** (§2.3.8)
9. **No exit-code change** — 0078 owns that (§2.3.9)
10. **Corruption we recovered from is still corruption** — a block used despite a failed CRC taints the item that read it; suspect bytes never compute identity (§2.3.10, §3.3a)
11. **No new workspace dependency** — `tracing-throttle` declined (§3.2)
12. **Split-only on corrupt sources** — `CRC_SUSPECT` may only *refine* 0076 groups, never merge; Tier 1 is untouched (§3.3a, DoD-12)

## Phase 0 — Baseline + preconditions → DoD-12 foundation

- [ ] `ledgerful ledger start …`; `ledgerful scan --impact`; read `.ledgerful/reports/latest-impact.json`
- [ ] Confirm `main` clean at/after `79b5cdf`
- [ ] **Capture the pre-0077 baseline before any edit**: messages written, unique counts, keep-set winners, `content_hash_hex` for `fixtures/aspose_outlook.pst` + `promotions_spam.pst` — this is what DoD-12 diffs against
- [ ] Record locked `tracing` 0.1.44 / `tracing-subscriber` 0.3.23 (≥0.3.20 ANSI escaping) in `review.md`; no pin change

## Phase 1 — `integrity_telemetry` module → DoD-1

- [ ] `pst-reader/src/integrity_telemetry.rs`: thread-local `Cell<u64>` counters, global `AtomicU64` flush, bounded distinct-BID set (cap 1024) + `exact` flag
- [ ] Count **reads** as well as mismatches (`page_reads`, `block_reads`) — without a denominator no threshold in §3.5 is interpretable
- [ ] `snapshot` / `delta_since` / `reset` / `set_log_limit` / `flush_summary`
- [ ] Module doc comment states rule 4 (CRC stays warning-only) and the global-state test constraint
- [ ] `TEST_LOCK: Mutex<()>` + `reset()` for serialized telemetry tests — no new dependency
- [ ] Unit tests: counting, delta, cap → `exact=false`, reset

## Phase 2 — Route the warn sites + bound emission → DoD-2, DoD-3

- [ ] `page.rs:109`, `block.rs:80`, `block.rs:101` → `note_page_crc` / `note_block_crc` / `note_block_bid_mismatch`
- [ ] Gate: first *N* (default 10) per category, then ≤1 aggregate per interval (default 30 s), then final flush
- [ ] Test: ≥10,000 mismatches → emitted lines bounded **and** total exact (the two assertions together are the point)
- [ ] Grep-assert no CRC `tracing::warn!` survives outside the gate

## Phase 3 — Corrupt fixture → DoD-10 (closes D-0074-crc-fixture)

- [ ] Generate with `pst-writer` into a `tempfile`, flip bytes in a known page + a known block trailer
- [ ] Deterministic, separately-asserted counts for page CRC / block CRC / BID mismatch — assert the *specific* counters, not "a warning happened"
- [ ] Only if generation is too slow for the unit suite: commit a small synthetic corrupt PST under `fixtures/`. **Never** derived from a real file

## Phase 3a — Message-level `CRC_SUSPECT` → DoD-19..22 (fixes D7)

> The correctness payload of this track. Sequenced after the fixture because it is untestable before it.

- [ ] `IntegrityReason::CrcSuspect` → `"CRC_SUSPECT"`; **do not** reuse `CrcMismatch` (skipped-for-CRC ≠ kept-despite-CRC)
- [ ] Snapshot the thread-local counters on entry/exit of `read_message_properties`, `read_message_extract`, and attachment stream reads; non-zero delta ⇒ `degraded` + `CRC_SUSPECT`
- [ ] Tier-2 ineligible by default (split-increasing ⇒ satisfies 0076 lock 1 without a flag); **Tier 1 untouched**
- [ ] `--allow-crc-suspect-tier2` restores pre-0077 grouping exactly
- [ ] Explicit arm in `keepset.rs::reason_fidelity_tier` — `graded_fidelity_rank` (`keepset.rs:1328`) takes the worst mapped tier, so an unmapped reason silently defaults. **Verify, do not assume**
- [ ] `crc_suspect_messages` on `FileScanStats` + `ScanSummary`, JSON and human summary
- [ ] Tests: tainted vs untainted message **in the same corrupt file** (proves scoped, not file-global); MID-bearing suspect twin still merges via Tier 1; clean copy wins under `graded`; `--allow-crc-suspect-tier2` reproduces pre-0077; 0076 refinement assertion holds (subset, never merge)

## Phase 4 — Scan wiring + attribution → DoD-4, DoD-5

- [ ] Snapshot/delta around each source in `scan.rs`; four counters onto `FileScanStats`, totals + `distinct_bad_bids_exact` + `block_crc_rate` onto `ScanSummary`; all `#[serde(default)]`
- [ ] Comment at the snapshot site naming **D-0077-parallel-attrib** so 0079 finds the constraint where it would break it
- [ ] Test: two sources, corruption in the second only → file[0] zero, file[1] non-zero
- [ ] Test: `crc_skip_rate` **unchanged** on a fixture with block CRC hits and zero message skips (this is the regression that matters)
- [ ] Pre-0077 `scan_integrity_v1` payload still deserializes

## Phase 5 — `export_risk` → DoD-6, DoD-7, DoD-8

- [ ] `ExportRisk { level: PreflightRecommendation, reasons, inputs, thresholds }` on `unique_export_report_v1`
- [ ] Inputs from existing data only: attach fail rate, `block_crc_rate`, degraded winner rate, `partial` / `failed_volume_index`, carried-forward scan recommendation
- [ ] Two threshold tiers: **advisory** (`max_attach_fail_rate` 0.05 reusing 0074's number, `max_block_crc_read_rate` 0.01, `max_degraded_winner_rate` 0.02) → `re_export_recommended`; **catastrophic** (`catastrophic_block_crc_read_rate` 0.15, `catastrophic_attach_fail_rate` 0.50) → `not_export_ready`
- [ ] Catastrophic thresholds key on `block_crc_read_rate` (a true `[0,1]` fraction), never on the per-message rate
- [ ] Composition = max(scan, post-export); export never lowers risk
- [ ] `reasons` closed-vocabulary and sorted, naming threshold and observed value (`block_crc_read_rate=0.203>0.15`)
- [ ] Tests: monotone composition; an advisory crossing (0.06 attach fail) cannot reach `not_export_ready`; a catastrophic rate (0.20 read rate, no failed volume) **does**; workspace grep finds no competing risk enum

## Phase 6 — Bounded event Vec → DoD-11 (closes D-0073-vec-events)

- [ ] Cap `WriteCounters::attachment_fidelity_events` at 1000 (first-N kept) + `_truncated` + `_total` on the report
- [ ] Confirm — do not assume — that the existing `writer_fidelity` tests assert below the cap
- [ ] 0073 CSV ledger path unchanged; it remains the record of legal interest

## Phase 7 — CLI + summaries → DoD-9, DoD-13

- [ ] `--crc-log-limit` / `--crc-log-interval-secs` on `scan`, `dups`, `keep-set`, `unique-eml`, `unique-pst`; identical names + help
- [ ] Update **both** parsers (`main.rs`, `unique_pst_cmd.rs`) and their error messages together
- [ ] One human-summary line on scan and unique-pst: counts, distinct BIDs, exactness, `export_risk` — **numbers only**
- [ ] Hostile-folder-name test: `\x1b[31m…` in a fixture produces no such bytes on the new lines
- [ ] **Desk wizard (DoD-23):** `export_risk` onto `UniqueOutcomeView` (already a GUI subset of `UniquePstOutcome`); in `views/unique_wizard.rs::show_done` (line 354) the green "Export completed successfully." (line 374) fires **only** when level is `ok`, else a yellow/red banner + a `unique_done_stats` row. Unit-test the outcome→banner mapping, not egui rendering
- [ ] `--help` snapshot updated; `cargo check -p pst-dedup-gui`

## Phase 8 — Compatibility + performance → DoD-12, DoD-16

- [ ] **Clean**-fixture run diffed against the Phase-0 baseline: messages, unique counts, winners, `content_hash_hex` byte-identical (no CRC hit ⇒ no taint ⇒ no change)
- [ ] **Corrupt**-fixture run: only permitted delta is `CRC_SUSPECT` items leaving Tier 2, proven split-only by the 0076 refinement assertion and accounted for by `crc_suspect_messages`
- [ ] Page-heavy fixture scan timed before/after → `review.md` (≤2% target, +5% ceiling)

## Phase 9 — Docs → DoD-14, DoD-15

- [ ] `docs/unique-pst-export.md` integrity section + decision tree (§3.8 table)
- [ ] **ScanPST modifies the file it repairs** (writes `.bak`) → run on a **copy**; repairing evidence in place is a chain-of-custody event
- [ ] **ScanPST repairs by deleting what it cannot recover** — "Repair complete" ≠ "nothing lost". Lead the runbook with this; it is the workflow's most dangerous misreading
- [ ] Give the concrete count-diff: `pst-dedup scan <original> --json` vs `pst-dedup scan <repaired-copy> --json`, compare `total_messages` + per-folder counts, log any drop as disclosed data loss with the delta stated
- [ ] ScanPST ships with **classic** Outlook (M365 / 2024 / 2021 / 2019 / 2016) — do not assume it exists on "new Outlook"
- [ ] Purview: re-export with *"Also include items that have an unrecognized format, are encrypted, or weren't indexed"* and read the **unindexed items report** before calling a PST corrupt
- [ ] **Unindexed ≠ corrupted:** CRC block errors are *physical* byte corruption (remedy: re-download / re-export); Purview unindexed items are *logical* indexing exceptions in a byte-perfect file — password-protected, unsupported format, oversized (remedy: decrypt, different extractor, or documented exclusion). Wrong remedy = days lost
- [ ] Purview PSTs reported opening empty / without `Top of Information Store` at correct byte size → **check folder and message counts, not file size**
- [ ] State that this tool never repairs a source; remediation is re-export or a repaired copy
- [ ] `docs/audit.md` SEC-06 → "warning-only **and counted per source**"; **not** claimed closed
- [ ] Cross-link 0078 (exit codes) and 0081 (runbook)

## Phase 10 — Gate + finalize → DoD-17, DoD-18

- [ ] Targeted tests, then `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo test --workspace`
- [ ] `ledgerful impact`; `ledgerful verify`
- [ ] Purge anything written under `output\`
- [ ] `review.md`; `D-0077-*` rows in `docs/deferred.md`; **mark D-0074-crc-fixture and D-0073-vec-events closed / 0077**; update the SEC-06 row
- [ ] `conductor.md` + `sequencing.md` → **Completed**
- [ ] `ledgerful ledger commit <tx-id> --summary "…" --reason "…"`

## Suggested order

1. Baseline **first** (worthless after an edit)
2. Telemetry module + gate — independently shippable; fixes the 246 MB flood on its own
3. Corrupt fixture — nothing after this is testable without it
4. **`CRC_SUSPECT` taint (Phase 3a)** — the correctness payload; if the track is cut short, this is what must survive
5. Scan wiring (the D2 fix: the metric that was blind)
6. `export_risk` → event cap → CLI + Desk banner → compat → docs → gate

## Handoff notes

- **Do not** make CRC fatal or redefine `crc_skip_rate` — 0077 reports better, it does not decide differently.
- **Do not** solve this in a `tracing` Layer; release Desk installs no subscriber, and the numbers must reach `summary.json`.
- **Do not** add `low|elevated|high` or any second risk vocabulary.
- **Do not** let a suppressed line go uncounted — that is data loss wearing noise control's clothes.
- **Do not** print PST-derived strings on any new line.
- **Do not** change an exit code (0078), and do not commit a corrupt PST derived from a real file.
- **Do not** reuse `CrcMismatch` for the taint, and **do not** let `CRC_SUSPECT` gate Tier 1 — a suspect message with a readable MID must still merge.
- **Do not** repair, re-hash, or guess at suspect bytes. Flag and move on.
- **Do not** add `CrcSuspect` without an arm in `reason_fidelity_tier` — unmapped reasons take a default tier silently.
- **Do not** let the wizard paint green success when the risk level is not `ok`.
- **Rollback:** `--crc-log-limit <huge>` restores the pre-0077 log stream; `--allow-crc-suspect-tier2` restores pre-0077 grouping exactly; counters and `export_risk` are additive and inert to writing.
