# 0082 — Recipient Table Fidelity — Plan

> Structure follows `C:\dev\coordinated\conductor\templates\0000-Description\plan.md`.
> Phased checklist; each phase maps to DoD items in `spec.md` §7. Execute in `C:\dev\Dedupe`.
> Mark items `- [x]` as completed.

> **Ledger:** open a transaction before starting —
> `ledgerful ledger start 0082-RecipientTableFidelity --category FEATURE --message "<intent>"`
> — and commit it in the final phase. (Planning / review-fold uses DOCS txs if separate.)

> **Revised 2026-07-29:** dual-AI review fold-in (EX identity, `bcc_suppressed`, zero-recip anomaly, DL honesty, template `0x692` + 14 columns).

---

## Phase 0 — Precondition / MS-PST gate → DoD-12

- [ ] Confirm board: 0073–0081 **Completed**; workspace builds (`cargo check -p pst-writer -p pst-reader -p dedup-engine -p pst-dedup-cli`).
- [ ] Confirm MS-PST **Recipient Table Template**: NID **`0x692`**, **14 MUST columns** (spec §2.2 table already verified 2026-07-29 — re-open the Learn page if >30 days stale and note access date in `review.md`).
- [ ] Inventory attach-table write helpers (`0x671` TC/RowIndex) to reuse for recipient TC machinery (different NID + column set).
- [ ] Grep for existing `Recipient` / `0x0C15` / `0x39FE` / `0x0E07` / `MSGFLAG` — avoid duplicate types; note message-flags surface for rule 8.
- [ ] Re-query crates.io maxes if planning date is stale (>7 days); expect **no bumps** (§2.4).
- [ ] `ledgerful ledger status --compact`; start FEATURE ledger tx for implementation.
- [ ] Re-read `spec.md` §2.5 locked rules and §2.9 Q1–Q12 — do not re-litigate.

## Phase 1 — Reader: structured recipients + flags → DoD-1

- [x] Add `Recipient` / `RecipientType` types in `pst-reader` (crate style; no `unwrap`/`expect` in prod).
- [x] Walk message subnode `NID_TYPE_RECIPIENT_TABLE` (0x12); parse TC rows for type + display + address_type + email + smtp (and optional binaries if cheap).
- [x] Implement `identity_key()` cascade preview unit-tested at reader or engine (§2.5 rule 4).
- [x] Surface `PidTagMessageFlags` / UNSENT bit enough for zero-recip anomaly (or explicit skip path if prop absent).
- [x] Missing/corrupt table → empty `Vec`, still return message (display_* unchanged).
- [x] **Do not** invent rows from `display_to`/`cc`/`bcc`.
- [x] Unit/integration tests: empty table; multi-row To/Cc/Bcc; EX-typed row without SMTP; display-only message (no table) stays empty vec.
- [x] `cargo test -p pst-reader` green.

## Phase 2 — Writer: template 0x692 + full columns + BCC gate → DoD-2, DoD-3

- [x] Emit recipient **template** at **`0x692`**, zero rows, **all 14 MUST columns** (§2.2).
- [x] Per-message subnode TC: one row per **included** recipient; empty TC always present; synthesize structural columns (`ObjectType=6`, Responsibility, RecordKey/EntryId/SearchKey patterns, etc.).
- [x] Optional extra column `PidTagSmtpAddress` (`0x39FE`) when known.
- [x] Wire `include_bcc_recipients: bool` through `WriteMessage` / production options (default `false`).
- [x] Default: write To+Cc only; omit Bcc rows and `PidTagDisplayBcc`.
- [x] Opt-in: write Bcc rows + `PidTagDisplayBcc` when source provided them.
- [x] Keep writing `PidTagDisplayTo` / `PidTagDisplayCc` as today.
- [x] Round-trip tests: writer → reader recovers types + SMTP and EX address keys; BCC on/off matrix; assert template node readable at `0x692`.
- [x] `cargo test -p pst-writer` green.

## Phase 3 — Pipeline + identity (SMTP + EX) → DoD-4, DoD-5

- [x] Propagate `recipients` through materialize / `CanonicalMessage` / unique-pst prep (dedup-engine).
- [x] Tier-2.5: if `recipients` non-empty, fingerprint from each row's **identity_key** (SMTP → EX DN → display) over **To+Cc+Bcc**, sorted/normalized; else existing display path.
- [x] **Required synthetic:** two messages with EX-only recipients (no `PidTagSmtpAddress`) that share LegacyExchangeDN but differ in display formatting → **merge**.
- [x] SMTP synthetic still covered; table-less fixture still hashes display strings.
- [x] Stats: table-sourced EX counts into X.500 telemetry where appropriate.
- [x] `cargo test -p dedup-engine` green.

## Phase 4 — CLI, ledger, contract, QC, retryable, anomaly → DoD-6, DoD-7, DoD-8, DoD-9, DoD-10

- [x] Add `--include-bcc-recipients` to unique-pst CLI (and shared args struct for GUI pass-through).
- [x] Help text: default OFF + disclosure rationale (one sentence).
- [x] **`export_messages.csv`:** add **`bcc_suppressed`** boolean; tests true when source BCC omitted on write, false when written or absent.
- [x] Summary: `bcc_suppressed_message_count`; `sent_message_with_no_recipients_count`.
- [x] Zero-recip anomaly: empty table + not UNSENT → count; empty + UNSENT → no count; missing flags → skip (no invent).
- [x] Flip `fidelity_contract_v1` `recipient_table` off `DroppedByDesign` → `Preserved` (honest reason string).
- [x] Document BCC: still `DroppedByDesign` unless flag.
- [x] QC: fixture assertion source vs output recipient structure (row count / types / keys) on multi-recipient sample (**written** set respects BCC filter).
- [x] Add `retryable: bool` to unique-export summary JSON + classification helper.
- [x] Tests: permanent failures → `retryable: false`; only approved transient classes → `true`.
- [x] **No new exit integers; no new export_risk enum values.**
- [x] `cargo test -p pst-dedup-cli` green.

## Phase 5 — Docs + deferred hygiene → DoD-11

- [x] Update `docs/pst-writer-fidelity-v1.md` recipient row (was "No"); note template `0x692`.
- [x] Update `docs/unique-pst-export.md`: flag; identity cascade (SMTP→EX→display); `bcc_suppressed`; DL non-expansion honesty.
- [x] Update `docs/unique-pst-ediscovery-runbook.md`:
  - BCC disclosure + **reviewer note** for near-duplicate messages with `bcc_suppressed=true`
  - `retryable` field (still no blanket retry exit 5)
  - Zero-recip anomaly meaning (telemetry, not auto-block)
  - **No DL expansion** clause
- [x] Update `docs/deferred.md`:
  - D-0080-recipient-table → **closed / 0082**
  - D-0076-recipient-table → **closed / 0082**
  - D-0068-04 recipient half → **closed / 0082** (named-prop residual remains)
  - D-0018-03 → **closed / 0082** (or narrowed if matter extract still Display-only — note honestly)
  - D-0080-bcc-policy → **decided / 0082** (opt-in write + suppress ledger)
  - D-0078-retryable → **closed / 0082**
  - Explicitly leave D-0073-promote / D-0073-eml / D-0080-cloud-attachments / D-0079-deterministic-key open
- [x] `CHANGELOG.md` `[Unreleased]` entry for 0082 (no version bump)

## Phase 6 — Full verification + finalize → DoD-13, DoD-14

- [x] `cargo fmt --all --check`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo test --workspace`
- [x] `cargo deny check`
- [x] `ledgerful verify` (or justified fallback + exact command) — ledger commit on finalize; verify via pre-push hooks
- [ ] Optional operator smoke (local PSTs only): unique-pst default vs `--include-bcc-recipients`; never commit client files.
- [x] Write `review.md`: MS-PST citations + access date, template `0x692` evidence, EX identity test evidence, `bcc_suppressed` samples, anomaly counters, deferred closes, declined items with reasons, dual-AI fold-in summary.
- [x] Update `../conductor.md`: 0082 status → **Completed**; Series M row accurate.
- [x] Tier-1: `CHANGELOG.md` `[Unreleased]` entry (no version bump — release cadence is batched).
- [x] Commit the ledger transaction in the execution repo.
- [x] Notify: unblocks cleaner Tier-2.5 on real multi-mailbox (incl. EX); Mode A promote remains next P1 residual if scheduled.

---

## Handoff notes

- **Irreversible / outward-facing:** none (no release cut, no remote push required by this track).
- **Behavior change to watch:** keep-set grouping can change for messages with readable recipient tables — especially **EX DN keys** that previously fell through to display noise.
- **Do not** invent recipient rows from Display* strings on the reader.
- **Do not** default-write BCC; **do** set `bcc_suppressed` when omitting.
- **Do not** expand DLs / query GAL.
- **Do not** expand into named props, Mode A promote, or deterministic store keys mid-track — split instead.
- **Do not** invent a new `export_risk` value for zero-recip; counter only.
- **Rollback:** feature is additive TC + optional flag + CSV column; if blocked, leave contract as DroppedByDesign and do not flip DoD-6.
- Prefer small modules + explicit errors (`miette` / crate `Result`); production forbids `.unwrap()` / `.expect()`.
