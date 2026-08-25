# 0093 — Writer Heap + Recipient Robustness

> Structure follows `templates/0000-Description/spec.md`. DoD is §7.

- **Track ID:** 0093-WriterHeapRecipientRobustness
- **Execution repo:** `C:\dev\Dedupe`
- **Governance:** this directory in `C:\dev\Dedupe\conductor\` (track registry: `../conductor.md`)
- **Plan-of-record reference:** `C:\dev\Dedupe-plan.md` → Series N
- **Cross-repo contract:** n/a
- **Status:** Completed (Codex luna r4 PASS, 2026-08-25)
- **Depends on:** 0068 · 0080 · 0082 (all **Completed**)
- **Spec authored:** 2026-08-25
- **Series:** N (Operator fidelity — INC0102784 post-0092)
>
> **Review fold-in (2026-08-25):** dual-AI Ready review (`opencode-review.md` + `agy-review.md`) incorporated below.
> Disposition of each claim is in §2.7. Strategy **B** is locked; 48-row count is not an invariant.
>
> **Evidence:** operator unique-pst `output/inc0102784-0092-full/` (4055 msgs). Uncommitted local
> writer fixes already unblocked the write; this track lands them with tests + residual policy.

---

## 1. Objective

Land production-writer heap robustness so real Permute mailboxes with multi-KB `DisplayTo` /
subject / MID / `message_class` strings and large recipient TCs **complete unique-pst without
heap-page hard-fail**, while making recipient-row truncation **budget-aware, To-first, and
ledger-honest** (QC `known_gap`, not surprise `defect`).

**Closes:** `D-0068-01` (non-body string subnode diversion, including `message_class`).

**Spawns / keeps residuals:**

- `D-0093-recipient-tc-multipage` — multi-page / subnode recipient TC (Strategy A deferred).
- `D-0093-attachment-tc-page` — same-class single-page overflow on attachment-table TC (NID `0x671`).

---

## 2. Context (read before starting)

### 2.1 Operator evidence (INC0102784 unique-pst, 2026-08-25)

| Signal | Observation |
|---|---|
| Pre-fix | Write aborted: `heap page overflow: 8192 > 8176` (large Display* inlined) |
| Local fix (uncommitted) | `MAX_HEAP_VALUE_SIZE` 3580→2048; `push_string_prop` subnode diversion for MID/subject/sender/Display*; recipient TC interim cap **48** (`tracing::warn!` only) |
| Truncate WARNs | **29** `recipient TC truncated…` lines in `run.log` (largest `total=136 kept=48`) |
| QC defect | `recipient_table` — src 136 vs out 48 on sampled message (`fidelity_contract_v1.recipient_table` is `Preserved`; `classify(name, present)` cannot see a cap event) |
| Display* | Truncate WARN claims DisplayTo/Cc/Bcc unchanged (strings diverted, not clipped) |
| `lib.rs` | Uncommitted diff is **better `try_alloc` overflow diagnostics**, not fixture `build_pc_v2`. Fixture-path `build_bth` remains unchecked-by-design. |
| Hygiene | Untracked `crates/pst-reader/examples/probe_out.rs` (operator NPMAP probe; hardcoded INC* path; `.expect`) **breaks `cargo fmt --all --check`**. Phase 0 must delete, move under `output/`, or promote to a proper ignored example. |

### 2.2 Live code snapshot (verified 2026-08-25)

| Surface | State |
|---|---|
| Uncommitted | `crates/pst-writer/src/production.rs` + `lib.rs` `try_alloc` diagnostics |
| `HeapBuilder` | **Single-page by design** (`lib.rs`: "A simple heap-on-node builder for a single-block HN"). `MAX_BLOCK_DATA` = 8176. HNPAGEMAP projection `4 + (n+1)×2` matches MS-PST §2.3.1.5. |
| Body path | Already diverted above `MAX_HEAP_VALUE_SIZE` |
| Helper | `push_string_prop` covers MID / subject / sender-email / DisplayTo / DisplayCc / DisplayBcc **per-value** vs 2048. `PID_TAG_MESSAGE_CLASS` is still `PcValue::String` inline (`production.rs` ~3315–3319). |
| Recipient TC | Single-page HN; cap `&rows[..48]` in source order; WARN only. Each row heap-allocates 7–8 entries. |
| Attachment TC | `build_attachment_table_tc` (`production.rs` ~3384–3396, schema ~4539): one filename HID + row per attach — **no cap, no byte budget**. Out of implementation scope here; residual required. |
| Contract | `recipient_table` = `ContractStatus::Preserved` (`fidelity_contract.rs`). BCC KnownGap precedent at `DroppedByDesign`. Capped fidelity-event Vec + uncapped counter precedent: `WriteCounters.attachment_fidelity_events`. |
| QC sample | `select_sample_indices` already includes `max_by_key(subject.len())` / `sender.len()` (0080 D-0068-01 narrowing). No `display_to.len()` stratum yet. |
| Deferred | `D-0068-01` still open (0080 only made longest-subject QC exercise the hard-fail class) |

### 2.3 Product locks

1. **No silent Display\* truncation** — prefer subnode diversion; keep full Display* when present.
2. **Strategy B locked** for recipient TC this track: budget-aware cap + fidelity event + summary counters + QC `known_gap`. Strategy A (multi-page / subnode TC preserving all rows) is **out** — residual `D-0093-recipient-tc-multipage`.
3. **`MAX_HEAP_VALUE_SIZE` = 2048 is a documented writer-implementation deviation**, not an MS-PST format necessity. See §2.4.
4. **Per-value 2048 cannot bound aggregate page usage.** Message PC helper strings must use a **cumulative / adaptive** heap budget (see §2.5).
5. **Recipient cap budgets bytes, not a row-count invariant.** 48 may remain a starting/max hint; the write must stop when projected single-page heap usage (row HIDs + HNPAGEMAP) would overflow, and the event must report the **actual** kept count. See §2.6.
6. **When truncation is necessary:** keep `MAPI_TO` (type 1) first, then `MAPI_CC` (type 2), then `MAPI_BCC` (type 3); stable within class. Do not rely on source row order.
7. **Contract versioning:** do **not** mid-v1 rewrite `recipient_table` Preserved to mean “all source rows.” See §2.6.
8. **`message_class` goes through the same diversion helper** as subject/Display* (D-0068-01 names it). Custom form classes exist in the wild.
9. Attachment-table TC overflow is **not implemented here** but **must** spawn `D-0093-attachment-tc-page` — silent “out of scope” without a deferred ID is forbidden.
10. No production `unwrap`/`expect`; miette `Result`.
11. CI fixtures only; INC* remains operator-local evidence.
12. Reuse `WriteCounters` capped-Vec + exact-counter shape (`attachment_fidelity_events` precedent). No new cross-crate sink.

### 2.4 Heap threshold vs MS-PST (locked documentation)

Microsoft Learn [MS-PST] (accessed **2026-08-25**):

| Rule | Spec text | Consequence for 0093 |
|---|---|---|
| Per-value HN max | §2.6.1.2.2 / §2.6.2.3.2: variable-size data **≤3580 ⇒ HN allocation**, **>3580 ⇒ subnode** (NID in `dwValueHnid`) | 3580 is the **format** per-value rule |
| Multi-block HN | §2.3.1.6: when an HN no longer fits one data block, a data tree spans multiple blocks (HNHDR / HNPAGEHDR / HNBITMAPHDR) | This is what Outlook does; `HeapBuilder` does **not** |
| Subnode strings | Readers dispatch on NID-type bits (§2.2.2.1). `pst-reader` `PropContext::resolve_value` already resolves subnode strings | Diverting 2049–3580-byte values to subnodes **plausibly works** but is **off the documented layout pattern** |

**Phase 0 decision (locked):** keep 2048 + divert as a **single-page HeapBuilder workaround**, documented in `docs/pst-writer-fidelity-v1.md` as a deviation. Do **not** restore 3580 semantics via multi-block HN in this track. Optional operator scanpst/Outlook smoke on a 2–3.5 KB-diverted fixture is evidence, not a CI gate.

The module-doc framing “inherent to the MS-PST format” **overstates** the 2048 choice — fix that language when landing the code.

**Shared residual research** (record on `D-0093-recipient-tc-multipage`, do not implement): multi-block HN is simultaneously the principled restore of 3580 per-value semantics **and** half of Strategy A (row matrix as subnode §2.6.2.4.4 / §2.3.4.4.2 + multi-block HN for per-row string HIDs + RowIndex BTH growth). HID `hidIndex` is 11 bits (max 2047 allocations per heap) — a no-cap design must respect that too.

### 2.5 Cumulative / adaptive string diversion (locked)

The uncommitted helper tests each property independently against 2048. Six helper-eligible strings at 1.5–2 KB plus `message_class` plus inline body/HTML still exceed 8176 **after** per-value diversion decisions; overflow then hits `encode_pc_value` / `build_pc_v2` with **no retro-divert**.

**Required:**

1. Track projected single-page heap usage while building the message PC (header + inlined values + BTH + HNPAGEMAP).
2. If inlining the next helper string (or the set as a whole) would overflow, divert to a subnode **regardless of individual size**.
3. Preferred hook: the probe heap already built for `MessageSize` (`production.rs` ~3425) — on overflow, escalate the largest remaining inline helper strings to subnodes and re-probe. Catch-and-retry is acceptable; silent hard-fail is not.
4. Body/HTML already divert at 2048; do not clip them to “make room.” Display* stay full.

**DoD-1 fixture** must be a message with **multiple** multi-KB helper strings (at least subject + sender + DisplayTo + DisplayCc in the 1.5–2 KB band), not a single >2 KiB DisplayTo.

### 2.6 Recipient TC Strategy B (locked)

**Cap policy**

- Default recommendation in the original draft (“B first if A > 1 PR”) is now a **lock**, not a Phase 0 choice.
- A **fixed 48-row slice is not a bound.** 48 rows with 100+ char display names still ≈ 40–60 KB of per-row strings — same abort, different corpus. INC* passing (`kept=48`) is empirical, not an invariant.
- Implementation: budget-aware stop (projected heap usage vs `MAX_BLOCK_DATA` / conservative HNPAGEMAP) **and/or** catch-and-retry with fewer rows. 48 may be a **starting maximum**, not the reported invariant.
- Truncation order: `To` > `Cc` > `Bcc` (agy), stable within class. Event carries **per-class kept/dropped** (opencode). Display* on the parent Message PC are **never** truncated.

**Contract / QC**

`classify("recipient_table", false)` today yields `FindingClass::Defect` because the entry is `Preserved`. That cannot distinguish cap-truncation from real loss.

Lock:

1. Keep `recipient_table` = **“TC present & schema correct”** `Preserved` (0082 meaning). Do not silently change mid-v1 to “all source rows preserved.”
2. Cap truncation is classified via a **writer-surfaced per-message truncate record** (kept vs source, per-class counts) plus a dedicated QC rule → `FindingClass::KnownGap` with honest detail (actual kept count, not a hardcoded “48”).
3. Prefer a sibling contract entry (e.g. `recipient_table_rows` `BestEffort`) **or** a dedicated QC branch that consults the writer event — Phase 1 of the plan picks one; both beat rewriting `recipient_table`.
4. **Decline** the predicate `out_recipients.len() == 48 && out is a strict subset of src` as the QC rule. Budget-aware kept may be **&lt; 48**; the writer event is the source of truth.
5. Unexplained row loss **without** a matching truncate event remains `Defect`.

**Telemetry (reuse attach-event shape)**

| Surface | Requirement |
|---|---|
| Summary / `summary.json` | `recipient_tc_truncated_messages: u64` and `recipient_rows_truncated: u64` (uncapped counters) |
| Per-message event | Reason `RECIPIENT_TC_TRUNCATED`; source count; actual kept; per-class To/Cc/Bcc kept/dropped. Capped Vec + exact total, same as `attachment_fidelity_events` |
| WARN | May remain for operators; **not** a substitute for the event/counters |

### 2.7 Dual-AI review disposition (2026-08-25)

| # | Claim | Source | Disposition | Spec landing |
|---|---|---|---|---|
| O1 | 2048 is a single-page workaround; 3580 is the format per-value rule; diverting 2049–3580 is off documented layout | opencode | **Agree (document + residual).** Decline implementing multi-block HN / restoring 3580 in this track | §2.3 lock 3; §2.4 |
| O2 | Per-value threshold cannot bound aggregate; DoD-1 fixture too weak; adaptive escalate+reprobe | opencode | **Agree** | §2.5; DoD-1 |
| O3 | 48-row cap budgets rows, not bytes; INC* is empirical | opencode | **Agree** | §2.6 cap policy; DoD-2 |
| O4 | Attachment-table TC (NID `0x671`) is same-class uncapped overflow | opencode | **Agree spawn residual.** Decline expanding 0093 to cap the attach table | §2.3 lock 9; `D-0093-attachment-tc-page` |
| O5 | `classify(name, present)` cannot see cap-truncation; need writer-surfaced kept/dropped + contract-version decision; per-class counts | opencode | **Agree** | §2.6 contract |
| O6 | Strategy A is real writer-format work (row-matrix subnode + multi-block HN + BTH + hidIndex 11-bit). B-first is right; record the sketch on the residual | opencode | **Agree; lock B** | §2.3 lock 2; §2.4 residual research |
| O7 | `message_class` is in D-0068-01 but not on the helper | opencode | **Agree — include** | §2.3 lock 8; Phase 1 |
| O8 | Snapshot: `lib.rs` is `try_alloc` diagnostics; `probe_out.rs` breaks fmt; §8 needs CLI unique-pst tests | opencode | **Agree** | §2.1; §8; DoD-4 |
| A1 | Cumulative heap budget in `push_string_prop` / `build_message_payload` | agy | **Agree** | §2.5 |
| A2 | Sort/filter `To` > `Cc` > `Bcc` before cap; Display* never truncated | agy | **Agree** | §2.3 lock 6; §2.6 |
| A3 | QC: truncated TC → `KnownGap` not `Defect` | agy | **Agree class.** Decline `out.len()==48 && subset` as the predicate | §2.6 contract items 2–4; DoD-2 |
| A4 | Ship Strategy B; spawn multi-page residual | agy | **Agree** | locks; `D-0093-recipient-tc-multipage` |
| A5 | Structured counters + optional `RECIPIENT_TC_TRUNCATED` ledger reason | agy | **Agree** (reuse attach-event shape; no new sink) | §2.6 telemetry |

**Declined / not in this track**

- Multi-block HN / restore 3580 per-value semantics (O1 opportunity; shared research only).
- Implementing attachment-table cap (O4) — residual only.
- Naive `&rows[..48]` as the shipped invariant (contradicted by O3).
- Mid-v1 rewrite of `recipient_table` Preserved to “all rows.”

---

## 3. In scope

1. Land heap diversion + documented 2048 threshold + **cumulative/adaptive** budget (MID / subject / sender / Display* / **message_class**).
2. Recipient TC **Strategy B**: budget-aware cap, To>Cc>Bcc, fidelity event + summary counters, QC `known_gap` via writer-surfaced truncate record.
3. Close `D-0068-01`. Keep `D-0093-recipient-tc-multipage` (with §2.4 research). Spawn `D-0093-attachment-tc-page`.
4. QC sample stratum: `max_by_key(display_to.len())` in `select_sample_indices`.
5. Docs: `docs/pst-writer-fidelity-v1.md` (2048 deviation + Strategy B) + unique-pst export note.
6. Phase 0 hygiene: `probe_out.rs` must not break `cargo fmt --all --check`.

## 4. Out of scope

- Nested embedded-message export (`0094` / `D-0067-embedded-depth`).
- Folder-tree QC shape (`0095`).
- PermissionType extract (`0096`).
- Body-cloud truncation ledger (`0097`).
- Cloud hydrate.
- Multi-page / subnode recipient TC (Strategy A).
- Multi-block HN / restoring 3580 per-value semantics.
- Attachment-table TC cap (residual `D-0093-attachment-tc-page`).

## 5. Preconditions & dependencies

- **P1:** 0082 recipient TC write path exists.
- *Verified:* local diversion unblocks INC0102784 full write (4055/4055). Do not regress that unblock.

## 6. Risks

| Risk | Mitigation |
|---|---|
| Cap silently drops counsel-visible TC rows | To-first + ledger event + QC KnownGap; Display* full |
| Subnode strings unread by some clients | Reader round-trip; optional scanpst-on-copy of a 2–3.5 KB-diverted fixture |
| Fixed 48-row cap re-trips heap abort on long names | Budget-aware stop; report actual kept |
| Aggregate helper strings still overflow 8176 | Adaptive escalate + multi-string DoD-1 fixture |
| Scope creep into multi-GB / multi-page TC | Strategy B locked; A is residual |
| Attach-table overflow is a silent known-unknown | Named residual `D-0093-attachment-tc-page` |
| `probe_out.rs` fails the fmt gate | Phase 0 delete / relocate |

## 7. Definition of Done

Complete only when ALL hold:

- [ ] **DoD-1 —** Oversized non-body strings (subject / sender / Display* / MID / **message_class**) divert to subnodes under a **cumulative** budget; no heap-page hard-fail on a fixture with **multiple** 1.5–2 KiB helper strings (not only one >2 KiB DisplayTo).
- [ ] **DoD-2 —** Recipient overflow is Strategy B: budget-aware cap (actual kept count in the event; 48 is not the invariant), To>Cc>Bcc, machine-readable truncate counters + `RECIPIENT_TC_TRUNCATED` event with per-class kept/dropped. Differential QC on a ≥136-row synthetic fixture is `known_gap`, **not** `defect`. Unexplained row loss without an event remains `defect`.
- [ ] **DoD-3 —** `D-0068-01` **closed**. `D-0093-recipient-tc-multipage` kept with §2.4 research notes. `D-0093-attachment-tc-page` spawned.
- [ ] **DoD-4 —** `cargo fmt --all --check` (no `probe_out.rs` on the fmt path) / clippy `-D warnings` / targeted writer + `unique_pst_qc_0080` + unique-pst **report** tests green. QC sample includes longest `display_to`.
- [ ] **DoD-5 — Recorded:** `review.md`; conductor **Completed**; ledger commit (`BUGFIX` or `FEATURE`).

## 8. Verification commands (reference)

```powershell
cargo fmt --all --check
cargo clippy -p pst-writer -p pst-dedup-cli --all-targets -- -D warnings
cargo test -p pst-writer
cargo test -p pst-dedup-cli --test unique_pst_qc_0080
cargo test -p pst-dedup-cli --test unique_pst
# operator (optional): unique-pst on INC0102784 pair; expect 0 heap overflow; recipient policy per DoD-2
```
