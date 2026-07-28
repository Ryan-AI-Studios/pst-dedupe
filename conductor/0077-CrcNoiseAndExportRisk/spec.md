# 0077 — CRC Noise Control & Export Risk Score

- **Track ID:** 0077-CrcNoiseAndExportRisk
- **Status:** Ready
- **Series:** L (unique export hardening)
- **Depends on:** 0065 (integrity/preflight types), 0073 (attach ledger), 0074 (attach probe preflight)
- **Blocks / feeds:** 0078 (exit codes consume `export_risk`), 0081 (operator runbook)

---

## 1. Objective

Turn PST corruption from an **unbounded log flood that no metric counted** into a **bounded, counted, reportable signal**, and give the operator one honest answer to "should I re-export this source before I trust the unique set?"

| Capability | Today | After 0077 |
|---|---|---|
| Page/block CRC mismatch | one `warn!` per occurrence, forever | first *N* per class, then periodic aggregate, then final flush |
| CRC volume | ~108k lines / ~246 MB stderr on INC | bounded by *N* + rate; totals preserved exactly |
| CRC in the report | **absent** — no counter anywhere | `page_crc_mismatches` / `block_crc_mismatches` / `distinct_bad_bids`, per source and total |
| Same signal in Desk | **silently dropped** (release GUI installs no subscriber) | counters reach the report regardless of subscriber |
| CRC hit while reading one message | invisible — bytes used anyway, item looks clean | item tainted `CRC_SUSPECT`, Tier-2 ineligible, ranked below its clean twin |
| Export risk | `preflight.recommendation` (scan-time only) | same vocabulary re-evaluated **post-export** as `export_risk` |
| Desk wizard on a risky export | green "Export completed successfully." | success qualified by risk level; banner when not `ok` |
| Event accumulation under a fail storm | unbounded `Vec` | capped + `truncated` flag (rolls in D-0073-vec-events) |
| Corrupt-PST test coverage | none — no fixture can produce a CRC warn | synthetic corrupt fixture (rolls in D-0074-crc-fixture) |

**Industry anchors.** Microsoft documents that the Inbox Repair tool (`SCANPST.EXE`) **modifies the data file** and writes a `.bak` alongside it — which makes "just run ScanPST" a chain-of-custody event, not a diagnostic. Microsoft Purview eDiscovery exports omit unindexed/corrupt items from the main PST unless the operator selects *"Also include items that have an unrecognized format, are encrypted, or weren't indexed"*, and community reports document exported PSTs that open empty or without `Top of Information Store` at the correct byte size. Both facts belong in the decision tree (§3.8) and neither is currently written down anywhere in this repo.

---

## 2. Context and defects

### 2.1 What exists today (verified at `79b5cdf`)

- `crates/pst-reader/src/ndb/page.rs:109` — `tracing::warn!("Page CRC mismatch at bid=…")` inside `PstPage::validate`, called on every B-tree page read. Warning-only by deliberate design (comment at `page.rs:104`), returns `Ok(())`.
- `crates/pst-reader/src/ndb/block.rs:80` — same for block trailers, inside `validate_block_trailer`; `block.rs:101` — `Block BID mismatch`.
- `crates/pst-dedup-cli/src/main.rs:756` — `init_tracing`: verbosity `0 => "warn"`. **The flood is on at default verbosity**; `-v` only makes it worse.
- `crates/pst-dedup-gui/src/main.rs:14` — `#[cfg(debug_assertions)] tracing_subscriber::fmt::init();`. A release Desk build installs **no subscriber at all**.
- `crates/dedup-engine/src/integrity.rs` — `IntegrityReason::CrcMismatch` exists, but it is only ever attached to **message-level skips**; `PreflightReport::crc_skip_rate` is computed from those skips.
- `PreflightReport` / `PreflightRecommendation { ok | re_export_recommended | not_export_ready }` ship in `scan_integrity_v1` and are already consumed by 0066 and 0074.
- `crates/pst-writer/src/production.rs:699` — `record_attach_event` pushes every event into `WriteCounters::attachment_fidelity_events` unconditionally, each holding owned `message_subject` + `attach_filename` Strings, even when a CSV sink is attached.
- `docs/audit.md` SEC-06 records "CRC mismatches are warning-only (intentional but noted)".

### 2.2 Defects

**D1 — Unbounded, undeduplicated warning stream at default verbosity.**
~108k `Page CRC mismatch` lines and ~1.6M total WARNs, ~246 MB of stderr, on the INC two-source run. Redirecting stderr is not a fix: the operator loses the signal entirely.

**D2 — The loudest failure signal in the run reaches no metric.**
The INC run recorded `skip_rate` **0** while emitting ~108k CRC warnings. CRC skips are a subset of skips, so `crc_skip_rate` was necessarily 0 as well — page/block CRC never becomes a `SkipRecord`, so it can never reach either rate. This is the substantive defect: the number the report tells operators to trust is structurally blind to block-level corruption. 0077 is a **reporting-correctness** track, not a cosmetics track.

**D3 — The same signal is invisible in Desk, and the wizard actively asserts the opposite.**
Because the counters do not exist and the release GUI installs no subscriber, a Desk operator running the wizard over a corrupt PST sees nothing at all. Worse than absence: `views/unique_wizard.rs:374` paints a green **"Export completed successfully."** — an unqualified success claim on a source that scan may already have flagged `re_export_recommended`. A missing indicator is a gap; a green check on a risky export is a wrong answer. Filtering inside a `tracing` Layer would fix the CLI and leave Desk exactly as blind — which is why §3.1 counts in the data path and §3.10 qualifies that screen.

**D7 — Recovered-from corruption silently poisons Tier-2 identity.**
CRC is warning-only: `PstPage::validate` returns `Ok(())` and `validate_block_trailer` returns `Ok(BlockId)` *after* warning, so the suspect bytes are used. A message whose body block failed CRC but still decoded is therefore exported with silent, localized corruption **and** carries no integrity flag — `body_incomplete` / `body_unavailable` are set from parse *errors*, never from CRC warnings.

The consequence lands in 0076. That message is Tier-2 eligible with a content hash computed over corrupt bytes, so it **cannot group with its clean twin in another mailbox**: unique count inflates, and the corrupt copy ships as a distinct "unique" document. 0075's fidelity ladder cannot rescue it either, because the copies never reach the same group for a winner to be chosen. Both of the guards built to prevent exactly this are blind to it. This is the most consequential defect in the track and the reason 0077 is not optional polish.

**D4 — A second risk vocabulary would fork an existing one.**
The placeholder spec proposed `export_risk: low | elevated | high`. `PreflightRecommendation { ok | re_export_recommended | not_export_ready }` already ships, is already persisted in `scan_integrity_v1`, and already means the same three things — while being *actionable* rather than adjectival. Shipping both recreates precisely the `member_tier`-vs-`bound_by` duplication that 0076 deleted (0076 §2.3.7).

**D5 — Unbounded accumulator under a fail storm.**
`attachment_fidelity_events` grows without limit. 366 failures is harmless; a source that fails every attachment is not. Same bug class as D1 — unbounded output under corruption — which is why it belongs here and not in 0079 (rolls in **D-0073-vec-events**).

**D6 — Nothing in `fixtures/` can produce a CRC mismatch.**
No test can prove a limiter, a counter, or a threshold. 0074 already deferred this as **D-0074-crc-fixture**; 0077 cannot meet its own DoD without it, so it is rolled in.

### 2.3 Locked product rules

1. **Count first, log second.** Every suppressed line is still counted exactly. Suppression that loses the number is not noise control — it is data loss. Totals are exact; only *emission* is rate-limited.
2. **Counters live in the data path, never in the log path.** No behavior may depend on a `tracing` subscriber being installed (D3). This forbids a subscriber-Layer solution as the primary mechanism.
3. **One risk vocabulary.** `PreflightRecommendation` is reused verbatim for `export_risk`. No `low|elevated|high` enum is introduced (D4).
4. **CRC semantics are unchanged.** CRC mismatch remains **warning-only and non-fatal**, exactly as `page.rs:104` documents. 0077 changes *what is reported*, never *what is accepted, rejected, or written*. A run that produced N messages before 0077 produces the same N messages after it.
5. **Bounded memory under corruption.** Every accumulator introduced or touched by this track gets an explicit cap plus a `*_truncated` / `*_exact` flag. An honest "≥1024, exact=false" beats an OOM.
6. **Sources stay read-only.** 0077 adds no repair path, and the runbook must state that `SCANPST.EXE` mutates its input and therefore runs on a **copy** only.
7. **New output lines carry counters, not content.** Any log or summary line 0077 adds emits numbers, BIDs, and closed-vocabulary codes — never subjects, filenames, or folder names. Untrusted PST strings are an ANSI-escape-injection surface on a terminal, and our `println!` summary path does not go through `tracing`'s escaping.
8. **Additive JSON, append-only CSV, `#[serde(default)]` on every new field** (0075/0076 precedent).
9. **Exit codes are 0078's.** 0077 must not change any process exit code.
10. **Corruption we recovered from is still corruption.** A block whose CRC failed and was used anyway taints the item that read it (§3.3a). "The parser did not error" is not evidence that the bytes are right, and identity must never be computed from bytes we know are suspect (0076 §2.3.2: never dedupe on a field we failed to read — reading it *wrongly* is worse than not reading it).

### 2.4 Deferred items rolled in

| ID | Why it belongs here |
|---|---|
| **D-0074-crc-fixture** (P3) | 0077's DoD is untestable without a synthetic CRC-corrupt PST. Building it once serves both tracks. |
| **D-0073-vec-events** (P3) | Same defect class as D1: unbounded accumulation under a failure storm. Cheap here (one cap + one flag); 0079 keeps only the channel-only redesign. |
| **SEC-06** (`docs/audit.md`) | Not closed — CRC stays warning-only by design — but its *observability* half is: the finding becomes "warning-only **and counted, per source, in the report**". Update the audit row rather than claiming a fix. |

---

## 3. Design

### 3.1 `pst_reader::integrity_telemetry` — count in the data path

New module in `pst-reader` (the crate that owns PST parsing, per CLAUDE.md boundaries).

```rust
pub struct IntegritySnapshot {
    pub page_crc_mismatches: u64,
    pub block_crc_mismatches: u64,
    pub block_bid_mismatches: u64,
    pub distinct_bad_bids: u64,
    pub distinct_bad_bids_exact: bool,
}

pub fn snapshot() -> IntegritySnapshot;
impl IntegritySnapshot { pub fn delta_since(&self, prev: &IntegritySnapshot) -> IntegritySnapshot; }
pub fn reset();                       // tests only
pub fn set_log_limit(first_n: u64, summary_interval: Duration);
pub fn flush_summary();               // end-of-source / end-of-run
```

**Counter storage:** `thread_local!` `Cell<u64>` on the hot path, flushed into process-global `AtomicU64`s at flush points. Per-thread accumulation is not premature optimization — it is what makes attribution survive 0079's parallel materialize, where a global-only counter would attribute a worker's corruption to whichever source the main thread happened to be on.

**`distinct_bad_bids`:** per-thread `HashSet<u64>` capped at **1024**; merged into a global capped set under a `Mutex` at flush points only (never on the hot path). Past the cap, counting continues and `distinct_bad_bids_exact` goes `false`. The metric exists to answer one question — *three bad blocks re-read 108k times, or 108k bad blocks?* — and those have opposite remediations (proceed vs re-export). A capped answer still answers it.

**Call sites** replace the three `tracing::warn!` calls with `integrity_telemetry::note_page_crc(bid, computed, stored)` / `note_block_crc(..)` / `note_block_bid_mismatch(..)`. Each increments, then consults the gate.

**Emission gate:** first `first_n` (default **10**) per category at `warn!`, then at most one aggregate `warn!` per `summary_interval` (default **30 s**) carrying running totals, then a final `flush_summary()` line. `first_n = u64::MAX` restores pre-0077 behavior for debugging.

**Global state and tests:** the counters are process-global by necessity (`validate_block_trailer` is a free function with no reader context, and threading one through the NDB layer is a large, risky refactor for a reporting feature). Consequence, stated rather than hidden: telemetry tests must not run concurrently with each other. Guard them with a module-local `static TEST_LOCK: Mutex<()>` and `reset()` — no new dependency, no `#[serial]` crate.

### 3.2 Declined: a `tracing` Layer

`tracing-throttle` (rate-limiting Layer with LRU signature eviction) and a hand-written `Layer` were both considered and **declined as the primary mechanism**:

- it fixes the CLI and leaves Desk exactly as blind (D3) — a release Desk build has no subscriber to attach a Layer to;
- suppressed events would be counted in the *log* pipeline, so the numbers could never reach `summary.json` without a second path anyway;
- it adds a dependency to satisfy a requirement the call-site helper satisfies for free (0076 precedent: no new dependency for a reporting feature).

A `Layer` remains genuinely useful for *third-party* consumers of `pst-reader` who want to throttle our other warnings — recorded as **D-0077-tracing-layer**, not built.

### 3.3 Per-source attribution

`scan.rs` snapshots before opening each source and after finishing it; the delta lands in `FileScanStats`:

```rust
#[serde(default)] pub page_crc_mismatches: u64,
#[serde(default)] pub block_crc_mismatches: u64,
#[serde(default)] pub block_bid_mismatches: u64,
#[serde(default)] pub distinct_bad_bids: u64,
```

Totals plus `distinct_bad_bids_exact: bool` roll up onto `ScanSummary`. Attribution is **exact while sources are processed sequentially**, which is true on every path 0077 ships. When 0079 parallelizes materialize, per-thread counters must be read on the worker that did the work — recorded as **D-0077-parallel-attrib** with a comment at the snapshot site so the constraint is discoverable at the point it would break.

### 3.3a Message-level CRC taint — `CRC_SUSPECT` (fixes D7)

The same snapshot-delta mechanism as §3.3, one level finer. `read_message_properties`, `read_message_extract`, and the attachment stream reads snapshot the **thread-local** counters on entry and compare on exit; a non-zero delta means suspect bytes entered this item.

This is why §3.1 chose per-thread counters. A global counter could not attribute a CRC hit to a message; threading a reader context down through the NDB layer to `validate_block_trailer` would be a large, risky refactor of the parser for a reporting feature. Per-thread deltas make item-level attribution nearly free — two `Cell<u64>` reads per message — and were already required for 0079.

**New reason code:** `IntegrityReason::CrcSuspect` → `"CRC_SUSPECT"`. Deliberately distinct from the existing `CrcMismatch`, which means *"we skipped this message because of CRC"*; `CRC_SUSPECT` means *"we kept this message and the bytes may be wrong."* Conflating them would hide the more dangerous of the two. The word is `SUSPECT`, not `RECOVERED` — nothing was recovered; the CRC failed and we used the block regardless.

**Attribution is deliberately over-inclusive.** A B-tree page walked *during* a message read counts toward that message even if the corrupt page held no part of it. That errs toward marking an item degraded that merely sat near corruption — never toward missing one. Over-tainting is split-increasing in 0076's terms (more items excluded from Tier 2 ⇒ more uniques, never a wrong merge), so it is safe under 0076's locked rule 1 and honest under this track's rule 10.

**Downstream effects, all following existing mechanisms:**

| Consumer | Effect |
|---|---|
| `RecoverableIntegrity` | `degraded = true`, `CRC_SUSPECT` appended to `degraded_reasons` |
| 0076 Tier-2 eligibility | ineligible by **default** — split-only, so it needs no new flag and breaks no 0076 lock. Tier 1 (MID) is **untouched**: a suspect message with a readable MID still binds by MID, which bounds the inflation to MID-less items |
| 0075 `--fidelity-rank graded` | needs an explicit arm in `keepset.rs::reason_fidelity_tier` (`graded_fidelity_rank` at `keepset.rs:1328` iterates `degraded_reasons` and takes the worst tier) so a clean copy outranks a suspect one. Binary mode already ranks it degraded. **Verify, don't assume** — an unmapped reason silently takes the default tier |
| Report | `crc_suspect_messages` on `FileScanStats` + `ScanSummary`; feeds `degraded_winner_rate` in §3.5 |

**Escape hatch:** `--allow-crc-suspect-tier2` (default off) restores pre-0077 eligibility for an operator who would rather have the merge than the split, with the count always reported either way.

### 3.4 `crc_skip_rate` stays; two new rates join it

`preflight.crc_skip_rate` keeps its exact current meaning (message-level CRC skips ÷ attempts) — changing it would silently move a threshold operators already tuned. D2 is fixed by *adding* the missing measurements, not by redefining the existing one. Two are needed, because they answer different questions:

```
block_crc_rate      = (page_crc_mismatches + block_crc_mismatches) / max(1, recoverable_messages)
block_crc_read_rate = (page_crc_mismatches + block_crc_mismatches) / max(1, page_reads + block_reads)
```

`block_crc_rate` is **corruption per document** — unbounded above (one message can trigger many block reads), useful for "how badly is this affecting my documents." `block_crc_read_rate` is a true fraction in `[0,1]` — **what share of reads failed CRC** — and it is the only one of the two on which a numeric threshold can mean anything, because "0.4" on the per-message rate and "40% of reads are failing" are different claims and only the second describes the medium. Thresholds in §3.5 therefore key on `block_crc_read_rate`; both rates and all raw counts are emitted. Total reads require one increment on each of the two already-touched paths.

### 3.5 `export_risk` — one vocabulary, two evaluation points

New object on `unique_export_report_v1`, typed with the **existing** enum:

```rust
pub struct ExportRisk {
    pub level: PreflightRecommendation,   // ok | re_export_recommended | not_export_ready
    pub reasons: Vec<String>,             // closed vocabulary, sorted, e.g. "attach_fail_rate=0.098>0.05"
    pub inputs: ExportRiskInputs,
    pub thresholds: ExportRiskThresholds,
}
```

**Inputs** (all already available post-export): `attach_fail_rate` = `attachments_failed / max(1, written + failed)`; `block_crc_rate` (§3.4); `degraded_winner_rate` from the keep-set; `partial` and `failed_volume_index` from `ExportSection`; `preflight.recommendation` carried forward from scan.

**Composition rule:** `export_risk.level` is the **max** of the carried-forward scan recommendation and the post-export evaluation. Export can only raise risk, never lower it — a clean export of a corrupt source is still a corrupt source.

**Thresholds** live in a `#[serde(default)]` struct so they are visible in the JSON and adjustable without a code read, in **two tiers** that are three-to-twenty times apart so nothing sits near a boundary:

| Tier | Threshold | Default | Produces |
|---|---|---|---|
| Advisory | `max_attach_fail_rate` | 0.05 (reuses 0074's number — same meaning, deliberately not a second knob) | `re_export_recommended` |
| Advisory | `max_block_crc_read_rate` | 0.01 | `re_export_recommended` |
| Advisory | `max_degraded_winner_rate` | 0.02 | `re_export_recommended` |
| **Catastrophic** | `catastrophic_block_crc_read_rate` | **0.15** | **`not_export_ready`** |
| **Catastrophic** | `catastrophic_attach_fail_rate` | **0.50** | **`not_export_ready`** |

`not_export_ready` otherwise requires a hard condition — a failed volume, `partial` with a failed volume index, or a carried-forward `not_export_ready`.

**Why a rate may reach a verdict after all.** The original rule here was "never a rate alone; a threshold crossing is advice, not a verdict." That reasoning holds at the boundary — 5.1% versus 4.9% must not be the difference between advice and refusal — but it fails at the extreme. A source where **15% of block reads fail CRC** is not a document set with a quality issue; it is a failing disk or a botched forensic image, and producing from it invites a defective-production or spoliation fight. So the rule is narrowed rather than dropped: *a verdict may not come from a hairline crossing of an advisory threshold, but it may come from a rate so extreme it is a fact about the medium.* The 3–20× gap between the tiers is what keeps that distinction real, and `reasons` always names which threshold fired at which value.

Naming: the operator-facing field keeps the placeholder's name `export_risk`; only the *values* are unified. Recorded decision in §3.9.

### 3.6 Bounded event accumulation (D-0073-vec-events)

`WriteCounters::attachment_fidelity_events` gains a cap (**1000**, first-N kept) plus `attachment_fidelity_events_truncated: bool` and `attachment_fidelity_events_total: u64` on the report. The CSV ledger from 0073 is the record of legal interest and is unaffected; the `Vec` exists for tests and for in-process consumers. Existing tests assert against small event sets and are unaffected by a 1000 cap — verify, don't assume.

### 3.7 Synthetic corrupt fixture (D-0074-crc-fixture)

Generate with `pst-writer` into a `tempfile`, then flip bytes in a known page and a known block trailer, producing deterministic page-CRC, block-CRC, and BID-mismatch hits. Prefer **generate-at-test-time** over committing a corrupt binary: it stays synthetic by construction, it is diffable as code, and it cannot be mistaken for evidence. If generation proves too slow for the unit suite, commit a **small** synthetic corrupt PST under `fixtures/` — never a real one, never a mutated copy of any operator file.

### 3.8 Operator decision tree (`docs/`)

New section in `docs/unique-pst-export.md` plus a compact decision tree. Grounded content only:

| Signal | Read it as | Action |
|---|---|---|
| `distinct_bad_bids` small, `page_crc_mismatches` huge | a few bad blocks re-read many times | usually proceed; check attach fail rate |
| `distinct_bad_bids` large / `exact=false` | widespread block corruption | re-export before trusting the unique set |
| `crc_suspect_messages` > 0 | these documents were **kept with possibly-wrong bytes** and held out of Tier 2 | expect a higher unique count than a clean run; they are flagged, not lost |
| `block_crc_read_rate` ≥ 0.15 | the medium is failing, not the mailbox | `not_export_ready` — re-image or re-export; do not produce |
| `attach_fail_rate` over threshold | attachment payloads unreadable | re-export; the 0073 ledger names which |
| `export_risk = not_export_ready` | a volume failed or scan already said so | do not hand off |

Must state plainly:

**ScanPST**
- **`SCANPST.EXE` modifies the file it repairs** (it writes a `.bak` first — Microsoft's own documentation). Run it on a **copy**, never on operator evidence. Repairing evidence in place is a chain-of-custody event.
- **ScanPST repairs by discarding what it cannot recover.** "Repair complete" does not mean "nothing was lost" — it means the file is now structurally consistent, which the tool achieves by dropping unrecoverable items. An operator who runs it on a copy, sees success, and ingests the result can silently lose hundreds of messages while believing they fixed something. This is the single most dangerous misreading in the whole workflow and the runbook leads with it.
- **Therefore: always diff the counts.** Concretely — `pst-dedup scan <original> --json` before, `pst-dedup scan <repaired-copy> --json` after, compare `total_messages` and per-folder counts. Any drop is data loss that belongs in the exception log disclosed to opposing counsel, with the delta stated. We are unusually well placed to make this cheap: producing honest before/after counts is what this tool already does.
- ScanPST ships with **classic** Outlook (Microsoft documents Outlook for Microsoft 365, 2024, 2021, 2019, 2016); do not assume it exists on a "new Outlook" machine.

**Purview**
- Before concluding a Purview export is corrupt: re-export with *"Also include items that have an unrecognized format, are encrypted, or weren't indexed"* and read the **unindexed items report**.
- **Unindexed ≠ corrupted — different problems, different fixes.** A CRC block mismatch is *physical* corruption in the PST's bytes; remedy is re-download or re-export. A Purview "unindexed item" is a *logical* indexing exception — a password-protected PDF, an unsupported format, an oversized spreadsheet — in a file that is byte-perfect; remedy is decryption, a different extractor, or documented exclusion. Re-downloading will never fix a password-protected attachment, and no amount of password work will fix a bad block. Operators routinely conflate the two and burn days on the wrong remedy. The table above keys off `distinct_bad_bids` and `block_crc_read_rate` precisely because those are the physical-corruption signals.
- Purview PSTs have been reported opening empty or without `Top of Information Store` at a correct byte size — **check folder and message counts, not file size**.

**This tool**
- Never repairs a source (project rule 3); every remediation is re-export or a repaired **copy**, and the repaired copy is a new evidence item with its own count delta.

### 3.9 Recorded decisions

| # | Decision | Reason |
|---|---|---|
| 1 | Reuse `PreflightRecommendation`; do **not** add `low\|elevated\|high` | Two vocabularies for one concept is the defect 0076 §2.3.7 removed; the existing words are also more actionable |
| 2 | Count at the call site, not in a `tracing` Layer | Release Desk installs no subscriber (D3); counters must reach `summary.json` |
| 3 | Decline `tracing-throttle` | No new dependency for a reporting feature; does not solve D3 |
| 4 | Keep `crc_skip_rate` meaning unchanged; add `block_crc_rate` | Silently redefining a shipped threshold breaks operators who tuned it |
| 5 | Export risk composes as **max**, never lowers scan risk | A clean export of a corrupt source is still a corrupt source |
| 6 | `not_export_ready` needs a hard failure **or a catastrophic rate**, never a hairline advisory crossing | A 5.1%-vs-4.9% crossing is advice; 15% of reads failing CRC is a fact about the medium, and calling that "advice" underplays a defective-production risk |
| 10 | Two rate denominators (`block_crc_rate`, `block_crc_read_rate`) | Only a true `[0,1]` fraction of reads can carry a meaningful numeric threshold; per-document corruption answers a different question |
| 11 | `CRC_SUSPECT` is a **new** reason, not reuse of `CrcMismatch` | One means "skipped because of CRC", the other "kept and possibly wrong" — conflating them hides the more dangerous case |
| 12 | Suspect items are Tier-2 ineligible **by default** | Split-increasing, so it satisfies 0076's split-only lock without a flag; Tier 1 untouched so MID-bearing copies still merge |
| 13 | Message taint via thread-local snapshot delta, not a reader-context refactor | Item-level attribution for two `Cell` reads; no parser surgery for a reporting feature |
| 14 | Desk risk banner is **in scope**, not deferred | The current screen asserts unqualified success; a wrong answer is not a missing feature |
| 7 | Per-thread counters even though 0077 is sequential | Attribution must survive 0079 rather than break silently |
| 8 | Generate the corrupt fixture at test time | Cannot be mistaken for evidence; diffable as code |
| 9 | CRC stays warning-only | Rule 4; changing acceptance is a different, larger track |

### 3.10 Flags and fields

| Surface | Name | Default |
|---|---|---|
| CLI (`scan`, `dups`, `keep-set`, `unique-eml`, `unique-pst`) | `--crc-log-limit <N>` | `10` (`0` = totals only; huge = pre-0077 firehose) |
| CLI (same) | `--crc-log-interval-secs <S>` | `30` |
| CLI (same) | `--allow-crc-suspect-tier2` | off (suspect items excluded from Tier 2) |
| `FileScanStats` | `page_crc_mismatches`, `block_crc_mismatches`, `block_bid_mismatches`, `distinct_bad_bids`, `crc_suspect_messages`, `page_reads`, `block_reads` | `0` |
| `ScanSummary` | the seven totals + `distinct_bad_bids_exact`, `block_crc_rate`, `block_crc_read_rate` | `0` / `true` / `0.0` |
| `UniqueExportSummary` | `export_risk: ExportRisk` | computed |
| `ExportSection` | `attachment_fidelity_events_truncated`, `attachment_fidelity_events_total` | `false` / count |
| `IntegrityReason` | `CrcSuspect` → `"CRC_SUSPECT"` | — |
| `UniqueOutcomeView` (Desk) | `export_risk: PreflightRecommendation` | carried from the summary |

Human summary gains one line on scan and on unique-pst: counts, distinct BIDs, exactness, and `export_risk` level — numbers only, per rule 7.

**Desk wizard (`views/unique_wizard.rs::show_done`, line 354).** `UniqueOutcomeView` is already a GUI-friendly subset of `UniquePstOutcome`, so this is one field plus one conditional widget:

- the green `"Export completed successfully."` at line 374 is emitted **only** when `export_risk.level == ok`;
- otherwise a yellow (`re_export_recommended`) or red (`not_export_ready`) banner states the level and points at `summary.json`;
- one row in the existing `unique_done_stats` grid carries the level either way.

In scope, not deferred: the failure mode is not that Desk lacks a feature, it is that Desk currently *asserts* success on a run the data says is risky. Richer surfaces — per-source counter tables, drill-down into `distinct_bad_bids` — are **D-0077-gui**.

---

## 4. Out of scope

| Item | Residual |
|---|---|
| Repairing PST pages/blocks in place | never (project rule 3) |
| Changing CRC acceptance semantics | rule 4 |
| Exit-code mapping from `export_risk` | **0078** — handoff in §8 |
| A reusable `tracing` rate-limit Layer for third-party consumers | **D-0077-tracing-layer** |
| Per-thread attribution proof under parallel materialize | **D-0077-parallel-attrib** (0079) |
| Installing a subscriber in release `pst-dedup-gui` / Desk | **D-0077-desk-subscriber** |
| Desk per-source counter tables / bad-BID drill-down (the banner itself ships) | **D-0077-gui** |
| Automating the ScanPST before/after count diff as a subcommand | **D-0077-repair-diff** — the runbook documents the two-command workflow; a `pst-dedup compare-counts` wrapper is a product decision |
| Re-hashing or repairing a `CRC_SUSPECT` body | never — we flag suspect bytes, we do not guess at correct ones |
| Full audit of untrusted strings on existing `println!` paths | **0081** dep/hygiene pass; rule 7 binds only new lines |
| Operator ScanPST run on a real corrupt PST | carries **D-0068-02** / **D-0071-operator-outlook** |
| Channel-only writer event path (no `Vec` at all) | **0079**; 0077 only caps it |

---

## 5. Preconditions

- `main` clean at or after `79b5cdf` (0076 merged). No rebase gate: 0076 already landed its `scan.rs` and `integrity.rs` changes.
- `tracing` 0.1.44 / `tracing-subscriber` 0.3.23 locked (verified in `Cargo.lock`). ≥0.3.20 is the version that escapes ANSI control characters in `fmt` output — relevant because PST-derived strings reach log lines. No pin change needed; record the check.
- No new workspace dependency.

---

## 6. Risks

| Risk | Mitigation |
|---|---|
| Global counters make tests order-dependent | `TEST_LOCK` + `reset()`; documented at the module head as a known cost of the design |
| Rate limiting hides a *newly* pathological source | Totals are always exact and always reported; the aggregate line fires every 30 s |
| `distinct_bad_bids` cap misleads | `exact: false` is emitted and documented in the decision tree |
| `export_risk` becomes a checkbox operators ignore | `reasons` names the crossing input and its threshold; `not_export_ready` needs a hard fact (§3.5) |
| Per-thread counters add hot-path cost | `Cell<u64>` increments, no atomics/locks on the hot path; measure page-heavy scan before/after |
| Byte-flipping the fixture accidentally produces a *different* failure | Assert the specific counter increments, not just "some warning happened" |
| Someone later "fixes" CRC by making it fatal | Rule 4 stated in spec, plan, module doc comment, and `review.md` |
| `CRC_SUSPECT` over-taints and inflates unique counts on a noisy-but-fine source | Split-only direction (never a wrong merge); Tier 1 untouched so MID-bearing copies still merge; `crc_suspect_messages` always reported; `--allow-crc-suspect-tier2` restores the merge |
| Message-scope snapshot delta misattributes an interleaved B-tree page read | Documented as deliberately over-inclusive (§3.3a); test asserts a message reading only clean blocks is **not** tainted while corruption exists elsewhere in the file |
| `CrcSuspect` added to `IntegrityReason` but not to `reason_fidelity_tier` → silent default tier | Explicit DoD-21 test asserting the graded rank of a suspect item beats/loses correctly vs a clean twin |
| Catastrophic threshold hard-fails a pipeline on a source the operator knowingly accepts | Threshold is in the `#[serde(default)]` struct and overridable; 0078 owns whether the level becomes a non-zero exit |
| Desk banner regresses the wizard's happy path | Green success path unchanged when `level == ok`; unit test on the mapping, not on egui rendering |

---

## 7. Definition of Done

- [ ] **DoD-1** `pst_reader::integrity_telemetry` exists: per-thread counters, global flush, bounded distinct-BID set with `exact` flag, `snapshot`/`delta_since`/`reset`/`set_log_limit`/`flush_summary`.
- [ ] **DoD-2** All three warn sites (`page.rs:109`, `block.rs:80`, `block.rs:101`) route through the gate. No `tracing::warn!` for CRC remains outside it.
- [ ] **DoD-3** Emission is bounded: first *N* per category, then ≤1 aggregate per interval, then a final flush. Proven by a test that produces ≥10,000 mismatches and asserts a bounded emitted-line count **and** an exact total.
- [ ] **DoD-4** Counters reach `ScanSummary` and `FileScanStats` per source, with `#[serde(default)]`; pre-0077 JSON still deserializes.
- [ ] **DoD-5** `block_crc_rate` **and** `block_crc_read_rate` added (with `page_reads` / `block_reads` denominators); `crc_skip_rate` **unchanged** — pinned by a test asserting its value on a fixture with block CRC hits and zero message skips. `block_crc_read_rate ∈ [0,1]` asserted.
- [ ] **DoD-19** `IntegrityReason::CrcSuspect` / `"CRC_SUSPECT"` exists and is set by message-scope snapshot delta on `read_message_properties`, `read_message_extract`, and attachment stream reads. Test: a message whose body block fails CRC is `degraded` with `CRC_SUSPECT`; a message reading only clean blocks **in the same corrupt file** is not tainted.
- [ ] **DoD-20** Suspect items are Tier-2 ineligible by default; Tier 1 (MID) binding is **unchanged** — test proves a suspect message with a readable MID still groups with its clean twin. `--allow-crc-suspect-tier2` restores pre-0077 eligibility exactly.
- [ ] **DoD-21** `CrcSuspect` has an explicit arm in `keepset.rs::reason_fidelity_tier`; test asserts a clean copy outranks a suspect copy under `--fidelity-rank graded` and under binary mode.
- [ ] **DoD-22** `crc_suspect_messages` reported per source and in total, in JSON **and** the human summary.
- [ ] **DoD-6** `export_risk` on `unique_export_report_v1`, typed `PreflightRecommendation`. **No new three-value risk enum exists in the workspace** (grep-checked in the test suite).
- [ ] **DoD-7** Composition is monotone: `export_risk.level >= scan preflight recommendation`, proven by a test where a clean export follows a `re_export_recommended` scan.
- [ ] **DoD-8** `not_export_ready` cannot be produced by an **advisory** threshold crossing — test at 0.06 attach fail rate. It **is** produced by a catastrophic rate — test at `block_crc_read_rate` 0.20 with no failed volume. `reasons` names the threshold and the observed value in both directions.
- [ ] **DoD-23** Desk wizard: green "Export completed successfully." appears **only** when `export_risk.level == ok`; a yellow/red banner plus a stats-grid row appears otherwise. Unit test on the outcome→banner mapping (not on egui rendering); `cargo check -p pst-dedup-gui`.
- [ ] **DoD-9** `--crc-log-limit` / `--crc-log-interval-secs` on all five subcommands with identical names and help; both parsers (`main.rs`, `unique_pst_cmd.rs`) updated together; `--help` snapshot updated.
- [ ] **DoD-10** Synthetic corrupt PST generated at test time yields deterministic page-CRC, block-CRC, and BID-mismatch counts (closes **D-0074-crc-fixture**).
- [ ] **DoD-11** `attachment_fidelity_events` capped at 1000 with `_truncated` + `_total` surfaced; existing `writer_fidelity` tests still pass (closes **D-0073-vec-events**).
- [ ] **DoD-12** Zero behavior change **on clean sources**: with no CRC hit there is no taint, so on the clean fixture corpus messages written / unique counts / keep-set winners / `content_hash_hex` are byte-identical to pre-0077 (rule 4). On a **corrupt** source the only permitted difference is `CRC_SUSPECT` items leaving Tier 2 — proven **split-only** (0076's refinement assertion: every post-0077 group is a subset of its pre-0077 group), never a merge, and fully accounted for by `crc_suspect_messages`. No message is dropped, no source byte changes, no exit code moves.
- [ ] **DoD-13** Every new log/summary line emits counters and closed-vocabulary codes only — no subject, filename, or folder path (rule 7); reviewed line by line in `review.md`.
- [ ] **DoD-14** `docs/unique-pst-export.md` decision tree per §3.8, including: ScanPST mutates the file / run on a copy; **ScanPST repairs by deleting unrecoverable items, with the explicit before/after `scan --json` count-diff workflow and the instruction to log any drop as disclosed data loss**; the classic-Outlook caveat; the Purview unindexed-items step; and the **physical corruption vs logical indexing exception** distinction with its two different remedies.
- [ ] **DoD-15** `docs/audit.md` SEC-06 updated to "warning-only **and counted per source**"; not claimed closed.
- [ ] **DoD-16** Performance: page-heavy fixture scan timed before/after; ≤2% target, +5% ceiling, recorded in `review.md`.
- [ ] **DoD-17** Gate green: `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo test --workspace`. `ledgerful verify`. `output\` purged.
- [ ] **DoD-18** `review.md` written; `D-0077-*` rows added to `docs/deferred.md`; **D-0074-crc-fixture and D-0073-vec-events marked closed / 0077**; `conductor.md` + `sequencing.md` flipped to Completed.

---

## 8. Verification

1. **Bounded-emission test** — 10,000+ synthetic mismatches; assert emitted lines ≤ `N + intervals + 1` and total == 10,000 exactly.
2. **Per-source attribution** — two-source scan, corruption in the second only; assert file[0] zero, file[1] non-zero.
3. **Backward compatibility** — a pre-0077 `scan_integrity_v1` payload deserializes; CSV header prefix unchanged.
4. **Vocabulary uniqueness** — test greps the workspace for a competing risk enum (DoD-6).
5. **No-behavior-change** — clean-fixture run diffed against the Phase-0 baseline; corrupt-fixture run checked against the 0076 refinement assertion (subset, never merge).
6. **Untrusted-string audit** — every new format string reviewed; test asserts a corrupt fixture with a hostile folder name (`\x1b[31m…`) produces no such bytes on the new lines.
7. **Taint precision** — in one corrupt file, a message over clean blocks is untainted while a message over the bad block is tainted; proves the delta is scoped, not file-global.
8. **Ladder integration** — clean vs suspect twin: Tier 1 still merges them, `graded` ranks the clean one winner, and with `--allow-crc-suspect-tier2` the pre-0077 Tier-2 grouping returns exactly.
9. **Risk tiers** — advisory crossing stays `re_export_recommended`; catastrophic rate reaches `not_export_ready` with no failed volume; `reasons` names threshold and value in both.

**Handoff to 0078:** `export_risk.level` is the intended exit-code input — `not_export_ready` → hard fail, `re_export_recommended` → partial-fidelity exit 3, `ok` → 0. 0077 ships the signal and changes **no** exit code.

**Handoff to 0081:** the decision tree in §3.8 is the runbook's integrity chapter; the `tracing-subscriber` ≥0.3.20 ANSI-escaping check belongs in the dep-pin audit.

---

## 9. Handoff notes

- **Do not** make CRC fatal, and do not change `crc_skip_rate`'s meaning.
- **Do not** implement this as a `tracing` Layer — Desk has no subscriber.
- **Do not** introduce `low|elevated|high`, or any second risk vocabulary.
- **Do not** let a suppressed line go uncounted.
- **Do not** print PST-derived strings on any new line.
- **Do not** change an exit code — that is 0078.
- **Do not** commit a corrupt PST derived from any real file; generate it.
- **Do not** reuse `CrcMismatch` for the taint — "skipped for CRC" and "kept despite CRC" must stay distinguishable.
- **Do not** let `CRC_SUSPECT` gate Tier 1; a suspect message with a readable MID must still merge, or the inflation is unbounded.
- **Do not** repair, re-hash, or guess at suspect bytes — flag them and move on.
- **Do not** add `CrcSuspect` to `IntegrityReason` without an arm in `reason_fidelity_tier`; an unmapped reason takes a default tier silently.
- **Do not** let the wizard paint green success when the risk level is not `ok`.
- **Rollback:** `--crc-log-limit <huge>` restores the pre-0077 log stream; `--allow-crc-suspect-tier2` restores pre-0077 grouping exactly; counters and `export_risk` are additive and inert to writing.
