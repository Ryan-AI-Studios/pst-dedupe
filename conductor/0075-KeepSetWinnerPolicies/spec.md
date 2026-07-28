# 0075 — Keep-Set Winner Policies (Date, Folder Class, Source Rank)

- **Track ID:** 0075-KeepSetWinnerPolicies
- **Execution repo:** `C:\dev\dedupe`
- **Governance:** this directory in `C:\dev\dedupe\conductor\`
- **Plan-of-record:** Series L — Unique export hardening (post-0072 / INC0102784 lessons)
- **Status:** **Ready** — spec expanded 2026-07-28 (research-backed); no code written yet
- **Depends on:** Hard **0066** (keep-set resolve / `rank_key` / decision CSV). Soft **0065** (integrity reasons), **0071** (unique-pst + report pack), **0073** (CSV-injection-safe writer conventions), **0074** (attach reason codes — required only for the *graded fidelity* item §3.6).
- **Downstream:** **0080** Outlook QC (winner sampling by class), **0081** operator runbook (which policy to run for which collection).
- **Priority:** **P1 Series L** — defensibility / operator-expectation gap, not a corruption gap.
- **Evidence:** INC0102784 keep-set — 598 winners crowned from `INC0102784-2.pst`, 3130 from the primary file, purely because `-2` sorts earlier by absolute path. Duplicate copies routinely live in `Recoverable Items\Purges` / `Versions`. Real PSTs stay operator-local; CI is synthetic only.
- **Deferred ledger:** append **D-0075-***. Never mutate source PSTs. Never change default winners without an explicit flag.

---

## 1. Objective

Make keep-set winner selection **explainable and steerable** instead of an accident of path sort order. Today the only tie-breaks after fidelity are scan order, size, or a boolean path match; operators cannot say *"prefer the live mailbox copy over the dumpster copy"* or *"prefer the primary collection file over the `-2` continuation"*, and the decision CSV cannot answer *"why did this copy win?"*.

| Capability | P0 |
|---|---|
| **`earliest_date` policy** | Sent-time (submit) primary, delivery fallback, missing sorts last (§3.3) |
| **Sender-copy completeness** | Opt-in BCC-bearing-copy rung — the sender's copy is the only one carrying BCC (§3.2.1) |
| **Folder-class preference** | Opt-in ladder: Sent Items > live mail > archive > junk/drafts > Deleted Items > Recoverable Items subtree (§3.4) |
| **Ordered source rank** | Ordered custodian/file preference — fixes the `-2` surprise; outranks folder class (§3.5) |
| **Explainability** | `decided_by` rung + class/date columns on every decision row (§3.7) |
| **Global-duplicate provenance** | Winner rows carry "All Custodians" columns in **CSV and JSON** (§3.7) |
| **Graded fidelity (opt-in)** | Rolls in **D-0066-fine-fidelity**; binary stays the default (§3.6) |
| **Zero silent change** | All flags default off ⇒ byte-identical winners vs pre-0075 (§3.9) |

**Outcome:** an operator can defend, per message, why one copy was exported and the rest suppressed — and can re-run with a documented preference ladder when opposing counsel or the case team disagrees with the default.

**Industry anchors (researched 2026-07-28):**

- **Relativity** (Processing / Deduplication considerations, RelativityOne 12.1, 2026-07): the email **Sent Date/Time from the message header** is the date used in the processing dedup hash; the retained "master" is the **first published copy**. First-seen is therefore a legitimate industry default — 0075 keeps it and adds alternatives rather than replacing it.
- **Global vs custodial dedupe** (Lexbe, GoldFynch, Digital War Room, Prosearch, 2025–2026): global (horizontal) dedupe is only defensible when the retained copy carries an **"All Custodians" / global custodian field** listing every source that held a duplicate. Our keep-set suppresses across files by default and currently records that only row-by-row on the loser side — §3.7 adds the winner-side aggregate.
- **Microsoft Learn — Recoverable Items folder in Exchange Online** (page updated 2026-07-13): the dumpster's subfolders are exactly **Deletions, Versions, Purges, Audits, DiscoveryHolds, Calendar Logging, SubstrateHolds**; `Versions` holds copy-on-write originals of *modified* items, `Purges` holds hard-deleted items, and the subtree is non-IPM (hidden from users).
- **Purview eDiscovery export**: Recoverable Items content **is** included in PST exports with "Include folder and path of the source", which is why dumpster copies show up as ordinary folder paths in our inputs and can win first-seen.

---

## 2. Context (ground truth)

### 2.1 What exists today

| Layer | State (verified in tree 2026-07-28) |
|---|---|
| Rank | `dedup_engine::keepset::rank_key` → `(fidelity_rank, policy_key, path_key, nid)` |
| Fidelity | `fidelity_rank` is **binary**: `degraded \|\| is_orphaned` → 1, else 0 |
| Policies | `KeepPolicy::{FirstSeen, KeepLargest, PreferPath}`; `FirstSeen` = `scan_order` = sorted-absolute-path order |
| `prefer_path` | **Unordered boolean**: any pattern match → 0, no match → 1 (all patterns equal weight) |
| Scan item | `RecoverableScanItem { locus, message_id_norm, content_hash, size, integrity, scan_order }` — **no date field** |
| Reader | `MessageProperties` has `submit_time`; **no** `delivery_time` (only `ExtractedMessage` has it) |
| Folder path | `FolderInfo::path` = `/`-joined **display names** from the root folder down (e.g. `Top of Personal Folders/Recoverable Items/Purges`) |
| Decision CSV | `DecisionRecord` carries role/tier/winner locus/policy/degraded reasons — **no** "which rung decided" column |
| Keep-set JSON | `KeepEntry` has locus/MID/hash/MIH/integrity/size/`promoted_from_failure` — **no** duplicate-source aggregate |
| Surfaces | `keep-set`, `unique-eml`, `unique-pst` CLI all take `--policy` / `--family-policy` / `--prefer-path-contains`; Desk wizard has a 3-item policy `ComboBox` |

### 2.2 Product rules (LOCKED)

1. **No silent behavior change.** With every 0075 flag absent, resolve must produce the **same winners** and the same values in pre-existing CSV/JSON fields as pre-0075. Regression-tested (§3.9).
2. **Default policy stays `first_seen`.** It matches Relativity's "first published copy is master"; changing the default is a separate product decision.
3. **Never invent a date.** File mtime, `PidTagLastModificationTime`, and "now" are forbidden as winner dates. Missing/zero/negative FILETIME = missing.
4. **Determinism first.** Every ladder rung must terminate in `(path_key, nid)`; two runs over the same inputs produce identical winners.
5. **Explain every decision.** Any rung that can decide a winner must be nameable in the decision CSV (`decided_by`).
6. **Source PSTs read-only.** No repair, no re-sort of input files on disk.
7. **CSV columns append-only.** New decision/report columns go at the **end**; existing column order and meaning are frozen for 0071/0073 consumers.
8. **Additive JSON.** `keep_set_v1` schema id is retained; new fields are additive and `skip_serializing_if`-guarded where empty.
9. **Closed vocabularies** for new enum-ish columns (`decided_by`, `folder_class`, `date_source`); free text from PSTs (folder names) goes through the existing CSV-injection-safe writer (0073 convention).

### 2.3 Deferred roll-in

| Item | Action in 0075 |
|---|---|
| **D-0066-fine-fidelity** — multi-level fidelity rank (soft reasons vs body/attach loss) | **Rolled in, opt-in** (§3.6). Binary remains default so rule 1 holds. Needs 0074 attach reasons to be worth enabling. |
| **"All Custodians" gap** (industry, not previously tracked) | **Rolled in** (§3.7) — winner-side duplicate source aggregate in **CSV + JSON**. |
| **Sender-copy / BCC loss** (raised on review 2026-07-28) | **Rolled in** (§3.2.1 rung + `sent_items` ladder rung §3.4 + `winners_without_bcc_peer_had_bcc` stat). |
| **D-0075-locale** (created by this spec) | **Partially pre-mitigated** — segment globs in `--folder-rank` (§3.4) ship in P0; the locale-proof fix stays deferred. |
| **0075 spec item "document first_seen = path order"** | **Rolled in** (§3.10) — docs + `--help` + a named `path_order` rung. |
| Custodial (vertical) dedupe scope | **Out** — grouping change, not winner change → **D-0075-scope** (§4). |
| Store-EntryID-based Deleted Items detection (`PidTagIpmWastebasketEntryId`) | **Out** — reader work, no operator pain yet → **D-0075-storeids**. |
| Localized folder-name packs (non-English mailboxes) | **Out** — `--folder-rank` override covers it → **D-0075-locale**. |
| D-0073-basename path redaction | Stays with **0081**; new columns must honor it if it ships. |

---

## 3. In scope

### 3.1 Placement (LOCKED)

| Component | Location |
|---|---|
| Policy enum + ladder + rank keys | `dedup-engine` `keepset.rs` (`KeepPolicy`, `rank_key`, new `RankContext`) |
| Folder classification | `dedup-engine` `keepset.rs` — pure function over `folder_path`, no PST I/O |
| Date + BCC capture | `pst-reader` `MessageProperties.{delivery_time, display_bcc}` (additive; `PID_TAG_MESSAGE_DELIVERY_TIME` 0x0E06 + `PID_TAG_DISPLAY_BCC` 0x0E02 already defined) → `pst-dedup-cli` `scan.rs` → `RecoverableScanItem`. **Both are extra `get_*` calls on the property context `read_message_properties` already loads — zero extra I/O.** |
| Decision/keep-set columns | `dedup-engine` `keepset.rs` (`DecisionRecord`, `KeepEntry`, `DecisionCsvWriter`) |
| CLI flags | `pst-dedup-cli` `main.rs` (`keep-set`, `unique-eml`) + `unique_pst_cmd.rs` |
| Desk wizard | `pst-dedup-gui` `views/unique_wizard.rs` + `unique_wizard.rs` form state |
| Docs | `docs/unique-pst-export.md` + keep-set section; 0081 cross-link |

**No new crate. No PST I/O added to `dedup-engine`.** Folder classification is string work on data the scan already carries.

### 3.2 The rank ladder (LOCKED)

```text
rank_key(item) = (
    fidelity_rank,        // §3.6 — binary today; graded opt-in
    bcc_rank,             // §3.2.1 — always 0 unless --prefer-bcc-copy
    source_rank,          // §3.5 — always 0 unless --source-rank
    folder_class_rank,    // §3.4 — always 0 unless --prefer-folder-class / --folder-rank
    policy_key,           // §3.3 — first_seen | keep_largest | prefer_path | earliest_date
    path_key,             // existing deterministic tail
    nid,                  // existing deterministic tail
)
```

- Lower is better at every position.
- New terms are **constant 0** when their flags are absent — this is what makes rule 2.2.1 hold.
- **`--source-rank` outranks `folder_class` by default** (changed 2026-07-28 on review). Rationale: `--source-rank` is an **explicit operator instruction** (they typed the ordered custodian/file list), while the folder ladder is a **heuristic**. Explicit beats implicit. The concrete case: producing for a CEO, the CEO's archived copy should beat a junior custodian's Inbox copy — under folder-class-first it would not.
- **One documented inversion:** `--rank-folder-class-first` swaps those two adjacent rungs for collections where folder provenance matters more than custodian identity. No general reordering knob — a single boolean swap stays testable and `decided_by` still explains every outcome.
- Everything above `policy_key` is a property of the **evidence** (is this copy intact? complete? whose? from where?); `--policy` is only the tie-break preference among equals.
- Signature change to `rank_key` is expected; introduce a small `RankContext { policy, prefer_path, bcc_mode, source_ladder, folder_ladder, folder_class_first, fidelity_mode }` value instead of growing the parameter list (call sites: `resolve_groups`, `finalize_with_materialize`).

### 3.2.1 Sender-copy completeness — BCC rung (LOCKED)

`--prefer-bcc-copy` (opt-in): `bcc_rank = 0` when the item has a non-empty `PidTagDisplayBcc`, else `1`.

**Why this rung exists:** in a Tier-1 (Message-ID) group, the sender's **Sent Items** copy and every recipient's **Inbox** copy are duplicates — but BCC recipients are stripped in transport, so **only the sender's copy carries BCC**. `earliest_date` cannot separate them (identical submit time) and neither can size reliably. Without this rung, `path_key` arbitrarily decides, and roughly half the time the export silently destroys the BCC evidence for that message. That is unrecoverable information loss, unlike custodian attribution (which survives in the duplicate-source provenance of §3.7).

**Placement — immediately after fidelity, above source rank:** BCC presence is an *evidence-completeness* signal of the same family as fidelity ("does this copy contain all the data?"), not a preference. In a one-copy-per-message unique set, completeness outranks whose copy it is.

| Aspect | Rule |
|---|---|
| Signal | `PidTagDisplayBcc` present **and** non-empty after trim |
| Cost | **Zero extra I/O** — one `get_string` on the property context `read_message_properties` already loads |
| Never | Do not infer BCC from recipient tables, folder name, or sender identity; absent = absent (0018 rule: never fabricate BCC) |
| Recorded | `has_bcc` column in the decision CSV; `decided_by = bcc_completeness` |

**Honest limits (document, do not paper over):** most messages have no BCC at all, so this rung is a no-op for the large majority of groups — it is a **targeted rescue**, not a general winner policy. It is also not a general "sender's copy" detector: a sender's copy with no BCC recipients is indistinguishable here. The broader sender-copy preference is handled by the `sent_items` rung in the folder ladder (§3.4), and the two are designed to be enabled together.

### 3.3 Policy `earliest_date` (LOCKED)

| Aspect | Rule |
|---|---|
| Primary date | `PidTagClientSubmitTime` (sent) — matches Relativity's dedup date convention |
| Fallback | `PidTagMessageDeliveryTime` (received) **only when submit is missing on that item** |
| Missing | Both absent, or FILETIME `<= 0` → treated as missing |
| Key | `policy_key = (has_date ? 0 : 1, filetime)` — **missing always sorts last**, never crowns an undated copy |
| Ties | Fall through to `path_key`, `nid` (unchanged determinism) |
| Recorded | `date_filetime_utc` (ISO-8601 UTC, empty when missing) + `date_source ∈ {submit, delivery, none}` |
| Cost | **Zero extra I/O** — `submit_time` is already read during scan; `delivery_time` is one more `get_time` on the already-loaded property context |

**Honesty note (must appear in docs and `--help`):** duplicate copies of the same message usually carry the **same** sent time, so `earliest_date` frequently ties and falls through to path order. Its real effects are (a) demoting copies that **lost** their date (common on `Versions` / dumpster copies), and (b) Tier-1 (Message-ID) groups whose members genuinely differ. Tier-2 groups **cannot** differ on submit time, because `submit_time` is already an input to the Tier-2 content hash — `earliest_date` is a no-op inside a pure Tier-2 group by construction. Do not market this policy as a general "keep the original" feature.

**Mixed-source comparison honesty:** when some members of a group resolve their date from `submit` and others from `delivery`, the comparison is apples-to-oranges. Do not suppress it (the alternative is discarding usable dates), but count it: `stats.groups_date_source_mixed` and note it in the human summary.

### 3.4 Folder-class preference (LOCKED)

Enabled by `--prefer-folder-class` (built-in ladder) or by supplying `--folder-rank` (custom ladder, **replaces** the built-in — no merge).

**Matching rules:**

1. Split `folder_path` on `/` into segments; compare **whole segments**, case-insensitive (never raw substring — a user folder named `Versions` must not be demoted by accident).
2. A pattern may be multi-segment (`Recoverable Items/Purges`); it matches when those segments appear **consecutively** in the path, with the last pattern segment being a real ancestor-or-self of the message's folder.
3. Recoverable Items subfolders are **parent-qualified**: `Purges` alone does not match; it must sit under a `Recoverable Items` segment.
4. A segment pattern may use `*` as a leading and/or trailing wildcard **within one segment** (`*Purges`, `*Element*`) — see "Wildcards" below. No `**`, no regex.
5. First matching rung wins; no match = rank 0 (best).

**Built-in ladder (default, best → worst):**

| Rank | `folder_class` | Matches |
|---|---|---|
| 0 | `sent_items` | segment `Sent Items`, `Sent Mail` |
| 1 | `primary` | anything not matched elsewhere (Inbox, user folders) |
| 2 | `archive` | segment `Archive`, `Online Archive`, `In-Place Archive*` |
| 3 | `junk_email` | segment `Junk Email`, `Junk E-mail`, `Spam` |
| 4 | `drafts` | segment `Drafts` |
| 5 | `outbox` | segment `Outbox` |
| 6 | `deleted_items` | segment `Deleted Items` |
| 7 | `recoverable_deletions` | `Recoverable Items` / `Deletions` (soft-deleted, user-recoverable) |
| 8 | `recoverable_holds` | `Recoverable Items` / `DiscoveryHolds` \| `SubstrateHolds` |
| 9 | `recoverable_purges` | `Recoverable Items` / `Purges` (hard-deleted) |
| 10 | `recoverable_versions` | `Recoverable Items` / `Versions` (copy-on-write **modified** originals) |
| 11 | `recoverable_ops` | `Recoverable Items` / `Audits` \| `Calendar Logging` (non-mail operational) |
| 12 | `recoverable_other` | any other child of `Recoverable Items` |

Recoverable Items rungs are anchored to Microsoft's documented subfolder set (§1 anchors).

**Ladder judgments (recorded so review can challenge them rather than infer them):**

- **`sent_items` above `primary`** — the sender's copy is the evidentially richest copy of a message (BCC, true send state). This is the general-case complement to the targeted BCC rung (§3.2.1); enable both together.
- **`drafts` / `outbox` below `junk_email`** — a draft or queued item is a **non-transmitted state** of the message. When Tier-2 groups a draft with the sent copy (or a user saved a received mail into Drafts), the transmitted copy must win rather than lose a path tie-break.
- **`junk_email` below `archive`** — still a genuine received copy, so it stays above the non-transmitted and deleted classes.
- **`recoverable_versions` below `recoverable_purges`** — a `Versions` item is by definition a **pre-modification copy** written by copy-on-write; it is the least representative of what the custodian held, and may be **structurally altered relative to its group peers** (Microsoft documents subject, body, attachments, participants, and sent/received dates as the properties that trigger copy-on-write). Note that its `submit_time` can still match the original, so `earliest_date` will **not** separate it — the folder ladder is the mechanism that does.

**Known behavior to document, not fix:** Outlook's ordinary IPM `Archive` folder shares the `archive` label and is therefore demoted below Inbox when the ladder is on. Intentional (prefer the live-folder copy), and part of why the ladder is opt-in.

**Custom ladder:** `--folder-rank <pattern>` repeatable, ordered **worst-last**; rank = `1 + index of first matching pattern`, unmatched = `0`. Supplying any `--folder-rank` **replaces** the built-in ladder (no merge).

**Wildcards (glob only — no regex):** `*` is permitted at the start and/or end of a segment pattern and matches within that segment only. This is the cheap mitigation for non-English mailboxes: `--folder-rank "*/Purges" --folder-rank "*Element*"` lets an operator demote a localized dumpster without enumerating exact translated root paths. Regex is deliberately **not** supported — it adds an untrusted-pattern audit surface for no gain over segment globs. Note the honest limit: a glob still requires the operator to know the translated folder name; the locale-proof fix is store-EntryID detection (**D-0075-storeids**), and wildcards are explicitly a workaround for **D-0075-locale**, not a closure of it. False positives from a broad glob are the operator's explicit instruction and are recorded in `folder_class`/`decided_by` like any other rung.

**Why the ladder is not silently always-on (reviewed and decided):** an always-on demotion of `Recoverable Items/Versions` was considered and **declined** — it would change existing operators' winner sets with no flag, breaking rule 2.2.1, which is the load-bearing safety property of this track. The safety benefit is delivered instead **without** changing winners: when any winner resolves to a Recoverable Items class, the run reports `stats.winners_from_recoverable_items` in JSON and prints a human-summary hint recommending `--prefer-folder-class` (see §3.7). Operators get the signal; the export stays predictable. Cross-link **0077** `export_risk`.

### 3.5 Ordered source rank (LOCKED)

`--source-rank <substring>` repeatable, ordered **best-first**, matched case-insensitively against the absolute source path (Windows: lowercased, same key as `path_compare_key`).

- rank = index of first matching pattern; **unmatched = `patterns.len()` (worst)**.
- Directly fixes the INC symptom: `--source-rank INC0102784.pst --source-rank INC0102784-2.pst` makes the primary file win every cross-file tie regardless of lexicographic order.
- This is also the **custodian-priority** lever: source files map to custodians, so `--source-rank CEO.pst --source-rank CFO.pst` states production priority directly. That is why it sits **above** the folder ladder (§3.2) — an explicit custodian list should not be overridden by a heuristic about folder names.
- `prefer_path` (existing, unordered boolean over `source_path|folder_path`) is **unchanged and not deprecated**; `--source-rank` is the ordered generalization for the source dimension only.

**Documented asymmetry** (goes in `--help` and docs as a table): `--folder-rank` enumerates *bad* places, so unmatched is **best**; `--source-rank` enumerates *preferred* files, so unmatched is **worst**. Both are tested for exactly this.

### 3.6 Graded fidelity — opt-in (rolls in D-0066-fine-fidelity)

`--fidelity-rank binary|graded`, **default `binary`** (today's behavior, unchanged).

| Graded rank | Meaning | Reasons (from `IntegrityReason`) |
|---|---|---|
| 0 | clean | none |
| 1 | soft / metadata only | `ATTACH_META_FAILED`, `ATTACH_PROBE_TRUNCATED`, `ATTACH_PEER_PROBE_CAP`, `ATTACH_PROBE_TIMEOUT` |
| 2 | attachment payload loss | `ATTACH_STREAM_OPEN_FAILED`, `ATTACH_STREAM_READ_FAILED`, `ATTACH_STREAM_CRC`, `ATTACH_BLOCK_NOT_FOUND`, `ATTACH_DATA_TRUNCATED`, `ATTACH_METHOD_UNSUPPORTED` |
| 3 | body loss | `BODY_TRUNCATED`, `BODY_UNAVAILABLE`, `DATA_TRUNCATED`, `CRC_MISMATCH`, `BLOCK_NOT_FOUND` |
| 4 | structural / provenance loss | `ORPHANED_NODE`, `INVALID_STRUCTURE`, `MESSAGE_READ_FAILED`, `PROPERTY_ERROR`, `NODE_NOT_FOUND` |

- Item rank = **worst** matching tier across its reasons.
- **Binary mode must map exactly**: `{0} → 0`, `{1,2,3,4} → 1` — proven by a test that runs the same fixture both ways and asserts binary winners are unchanged.
- Any reason added later without a tier mapping defaults to tier 3 (fail-worse, never silently "clean"), with a unit test asserting exhaustive mapping so new variants are caught at review time.
- Body loss outranking attach loss, and orphaned/structural being worst, are **product judgments** (evidence value of the message text vs its attachments; unknown folder provenance is unusable in a load file). Recorded for challenge.
- If **0074** has not landed when 0075 starts, the attach tiers are still declarable (reasons exist in `IntegrityReason` post-0074 only) — in that case this section drops to **D-0075-graded** rather than blocking the track.

### 3.7 Explainability & duplicate provenance (LOCKED)

**Decision CSV — appended columns (end of row, in this order):**

| Column | Values |
|---|---|
| `folder_class` | closed vocabulary from §3.4 (`primary` when ladder off) |
| `folder_class_rank` | integer |
| `source_rank` | integer (0 when `--source-rank` absent) |
| `has_bcc` | `true` \| `false` (§3.2.1) |
| `date_filetime_utc` | ISO-8601 UTC or empty |
| `date_source` | `submit` \| `delivery` \| `none` |
| `decided_by` | closed vocabulary (below) |
| `duplicate_source_count` | **unique rows only**: distinct other sources that held a suppressed copy (empty on dup rows) |
| `duplicate_sources` | **unique rows only**: `\|`-delimited distinct source names, capped (see below) |

`decided_by` vocabulary: `sole_member`, `fidelity`, `bcc_completeness`, `source_rank`, `folder_class`, `policy_first_seen`, `policy_keep_largest`, `policy_prefer_path`, `policy_earliest_date`, `path_order`, `nid`, `promoted_after_materialize_fail`.

- On a **`unique`** row: the rung at which the winner first beat its closest rival (`sole_member` when the group had one member).
- On a **`dup_of`** row: the rung at which **this** row first compared worse than its winner — i.e. "why I lost".
- Computed by comparing rank tuples that already exist; no extra passes, no extra I/O.

**Keep-set JSON winner (`KeepEntry`) — additive fields:**

| Field | Meaning |
|---|---|
| `folder_class` | as above |
| `decided_by` | as above |
| `duplicate_source_count` | distinct source PSTs (excluding the winner's own) that held a suppressed copy |
| `duplicate_sources` | sorted distinct `source_pst` names, **capped at 8** |
| `duplicate_sources_truncated` | `true` when the cap elided names |

**"All Custodians" parity must land in CSV, not only JSON (P0, raised on review).** Litigation-support teams ingest CSV/DAT directly into Relativity; if the *unique* row does not carry the duplicate sources inline, they must write a custom join from `dup_of` rows back to winners purely to build a load file. Therefore the same aggregate ships in **three** places:

| Surface | Columns/fields |
|---|---|
| Decision CSV (unique rows) | `duplicate_source_count`, `duplicate_sources` (`\|`-delimited) |
| `keep_set_v1` JSON winner | `duplicate_source_count`, `duplicate_sources` (array), `duplicate_sources_truncated` |
| `export_messages.csv` (`unique_export_report_v1`) | `duplicate_source_count`, `duplicate_sources` — appended after the columns 0073 added |

Rules for the aggregate (all three surfaces):

- **Value is the source PST name (basename), not the absolute path** — it identifies the custodian without pasting client directory structure into a load file (same privacy posture as the 0039 report scrub). If **D-0073-basename** ships a `--ledger-path-mode` switch, this column follows it rather than inventing a second knob.
- Cap **8** distinct names + `duplicate_sources_truncated`; keeps `keep_set_v1` bounded for multi-million runs (consistent with the D-0066-disk-groups RAM posture). The **uncapped** truth remains reconstructible row-by-row from the `dup_of` rows.
- `|` inside a quoted CSV field is valid; names still go through the 0073 CSV-injection-safe writer.

**Run-level honesty stats** (JSON summary + human summary): `winners_from_recoverable_items` (with the `--prefer-folder-class` hint when non-zero, §3.4), `winners_without_bcc_peer_had_bcc` (count of groups where a BCC-bearing copy lost — the exact loss §3.2.1 exists to prevent, and the number that tells an operator to re-run with `--prefer-bcc-copy`), and `groups_date_source_mixed` (§3.3).

### 3.8 CLI + Desk surface

```text
# keep-set / unique-eml / unique-pst all gain the same flags:
  --policy earliest_date            # new enum value alongside first_seen|keep_largest|prefer_path
  --prefer-bcc-copy                 # BCC-bearing copy wins (§3.2.1)
  --prefer-folder-class             # enable built-in folder ladder
  --folder-rank <PATTERN>           # repeatable, ordered worst-last; replaces built-in ladder; segment globs
  --source-rank <SUBSTRING>         # repeatable, ordered best-first (custodian priority)
  --rank-folder-class-first         # swap source_rank/folder_class rungs (§3.2)
  --fidelity-rank binary|graded     # default binary
```

- Parser errors must list the valid policy values (the existing `parse_keep_policy` message is updated in **all three** places: `main.rs` ×1 shared helper, `unique_pst_cmd.rs` ×1 — verify both are updated; today the message is duplicated).
- **Desk wizard:** add `earliest_date` to the policy `ComboBox` and a `Prefer folder class` checkbox. `--folder-rank` / `--source-rank` free-text lists are **residual D-0075-gui** — the wizard already passes `UniquePstCliArgs`, so defaults flow without UI churn.

### 3.9 Backward compatibility & determinism (LOCKED)

1. **Golden regression:** one test runs the existing fixture keep-set with **no** new flags and asserts (a) the winner set is identical to a checked-in golden list, and (b) the decision CSV's **pre-0075 columns** are byte-identical.
2. New CSV columns are appended; header constants get a paired test asserting the new header **starts with** the old header string.
3. `keep_set_v1` id unchanged; a test deserializes a pre-0075 keep-set JSON successfully.
4. Every new comparison terminates in `(path_key, nid)`; a shuffled-input test asserts identical winners.

### 3.10 Docs (LOCKED)

- `docs/unique-pst-export.md`: new "Winner policies" section — the ladder diagram (§3.2), the policy table, the folder-class ladder table, the `--folder-rank`/`--source-rank` asymmetry table, and the `earliest_date` honesty note (§3.3).
- Explicit statement (the original 0075 ask): **`first_seen` means sorted-input-path order, not chronological order** — with the INC `-2` example and the `--source-rank` remedy.
- `decided_by` / `folder_class` / `date_source` vocabularies documented as closed enumerations for downstream parsers.
- Cross-link **0081** runbook ("which policy for which collection") and **0080** QC (sample winners by `folder_class`).

### 3.11 Tests (LOCKED)

**`dedup-engine` unit (pure, no PST):**

1. `earliest_date`: earlier submit wins; missing date sorts last; delivery fallback used only when submit absent; equal dates fall through to `path_key`/`nid`.
2. Folder class: `Recoverable Items/Purges` loses to `Inbox`; a **user folder literally named `Purges`** (no `Recoverable Items` ancestor) is **not** demoted; segment matching rejects `MyVersions`; `Sent Items` beats `Inbox`; `Drafts`/`Outbox` lose to `Inbox` and to `Junk Email`; full built-in ladder ordering.
2b. **Sender-copy/BCC:** a Sent-Items copy with `PidTagDisplayBcc` beats an Inbox copy of the same MID under `--prefer-bcc-copy`; empty/whitespace BCC counts as absent; with the flag off, the pre-0075 winner is unchanged; `winners_without_bcc_peer_had_bcc` counts exactly the groups where a BCC copy lost.
3. `--folder-rank` custom ladder replaces built-in; unmatched = best; **segment globs** (`*Purges`, `*Element*`) match leading/trailing only and never cross a `/`.
4. `--source-rank` ordered: `INC.pst` beats `INC-2.pst` when ranked, loses when not ranked (the INC regression).
5. Ladder precedence: fidelity → bcc → source_rank → folder_class → policy; each rung proven by a pair that differs only at that rung. **Custodian-priority case:** ranked `CEO.pst` archive copy beats unranked `junior.pst` Inbox copy with both ladders enabled, and `--rank-folder-class-first` inverts exactly that pair and nothing else.
6. `decided_by` correct for each rung + `sole_member` + `promoted_after_materialize_fail`.
7. Graded vs binary: binary winners unchanged; graded prefers attach-loss copy over body-loss copy; exhaustive reason→tier mapping.
8. Duplicate-source aggregate: count/list/cap/truncated flag; identical values across decision CSV, `keep_set_v1` JSON, and `export_messages.csv`; basename-only (no absolute paths leak into the column).
8b. `winners_from_recoverable_items` counts correctly and the hint fires only when non-zero — **and winners are unchanged whether or not the hint fires** (proves the safety net is signal-only).
9. Determinism: shuffled input order → identical winners.

**`pst-dedup-cli` integration (synthetic only):**

10. Copy `fixtures/aspose_outlook.pst` to `a.pst` and `a-2.pst` in a temp dir; `keep-set` with and without `--source-rank` proves winner file flips deterministically, and **both source files hash-identical before/after** (existing immutability pattern).
11. Decision CSV header/`--help` snapshots for the new flags; JSON summary carries the new stats.

**Not required:** real INC PSTs, Outlook, ScanPST.

---

## 4. Out of scope

| Item | Why | Residual |
|---|---|---|
| Custodial / vertical dedupe scope (`--dedupe-scope per-source`) | Changes **grouping**, not winner choice; needs its own spec + report semantics | **D-0075-scope** (propose track) |
| Custodian roster / ranking UI in Desk | Desk work; CLI flags first | **D-0075-gui** |
| Store-EntryID-based special-folder detection | Reader change; keyword ladder is sufficient today | **D-0075-storeids** |
| Localized folder-name **packs** (shipped zh/de/fr/ja ladders) | `--folder-rank` **with segment globs** (§3.4) is the P0 workaround; a shipped pack is a separate data-curation job | **D-0075-locale** |
| Changing the default policy away from `first_seen` | Product decision, breaks rule 2.2.1 | — |
| Tier-1/Tier-2 grouping key changes | Owned by **0076** | — |
| Near-dup / thread collapse | Owned by matter jobs **0022** / **0023** | — |
| Family-level "master family" selection | Family policy is 0066/0067's `FamilyPolicy` | — |

---

## 5. Preconditions & dependencies

1. **0066** keep-set resolve is the substrate — `rank_key`, `ResolvedKeepSet`, `DecisionCsvWriter` are the edit surface.
2. **0074 must land first** (it is implemented but uncommitted on `track/0074-deep-attach-preflight` and touches `integrity.rs`, `scan.rs`, `pst_materializer.rs`, `unique_pst_cmd.rs`). 0075 edits `keepset.rs` (untouched by 0074) plus the same CLI files — **rebase after 0074 merges** rather than developing in parallel.
3. §3.6 graded fidelity requires 0074's attach reason variants; without them it degrades to **D-0075-graded**.
4. `pst-reader` gains two additive fields (`MessageProperties.delivery_time`, `MessageProperties.display_bcc`) — reader owns extraction, no policy leaks into the reader. Both tags are already defined in `ndb/nid.rs` and both reads land on the property context `read_message_properties` already loads.
5. Fixtures: existing `fixtures/aspose_outlook.pst` + temp-dir copies. No new committed fixtures, no client data.

---

## 6. Risks

| Risk | Mitigation |
|---|---|
| **Silent winner drift** for existing operators | Every flag defaults off + golden regression test (§3.9). An always-on `Versions` demotion was proposed on review and **declined** for this reason; replaced by the signal-only `winners_from_recoverable_items` hint (§3.4) |
| **BCC destroyed by crowning a recipient copy** (raised on review) | `--prefer-bcc-copy` rung + `sent_items` ladder rung + `winners_without_bcc_peer_had_bcc` stat so the loss is *visible* even when the flags are off |
| Custodian priority overridden by a folder heuristic | `--source-rank` sits above `folder_class` by default; `--rank-folder-class-first` for the inverse |
| Litigation-support forced to hand-join CSV rows for a load file | "All Custodians" columns on unique rows in decision CSV **and** `export_messages.csv`, not JSON-only (§3.7) |
| Broad `--folder-rank` glob demotes legitimate folders | Explicit operator instruction; recorded in `folder_class`/`decided_by`; built-in ladder stays exact-segment |
| False-positive folder demotion (user folder named `Versions`/`Purges`) | Whole-segment + parent-qualified matching; opt-in ladder; explicit test |
| Non-English mailboxes bypass the ladder entirely | Documented limitation + `--folder-rank` override; **D-0075-locale** |
| `earliest_date` oversold, then "does nothing" on Tier-2 groups | Honesty note in spec, docs, and `--help`; test that documents the no-op |
| Decision CSV column churn breaks 0071/0073 consumers | Append-only rule + header-prefix test |
| `rank_key` signature change ripples through call sites | Introduce `RankContext` in one commit before behavior changes |
| Merge conflict with in-flight 0074 | Sequence after 0074 merge (§5.2) |
| `duplicate_sources` unbounded on wide fan-out | Hard cap 8 + truncated flag; uncapped truth stays in decision CSV |
| Graded fidelity judgments (body vs attach vs orphan) disputed | Declared as judgments in §3.6; opt-in; challengeable in review without changing defaults |

---

## 7. Definition of Done

- [ ] **DoD-1** `KeepPolicy::EarliestDate` implemented with submit→delivery→missing-last semantics; `pst-reader` `delivery_time` **and** `display_bcc` captured through scan into `RecoverableScanItem` with **no extra PST I/O**.
- [ ] **DoD-1b** `--prefer-bcc-copy` rung per §3.2.1, placed directly after fidelity; `winners_without_bcc_peer_had_bcc` reported **whether or not the flag is set**.
- [ ] **DoD-2** Folder-class ladder implemented per §3.4 (whole-segment, parent-qualified, incl. `sent_items` / `junk_email` / `drafts` / `outbox`), enabled by `--prefer-folder-class`, overridable by ordered `--folder-rank` with segment globs.
- [ ] **DoD-3** Ordered `--source-rank` implemented with documented unmatched-worst semantics, ranked **above** `folder_class`, invertible via `--rank-folder-class-first`; INC-style regression proves the primary file can be preferred over `-2`.
- [ ] **DoD-4** Rank ladder is exactly §3.2, each rung independently proven, all comparisons terminating in `(path_key, nid)`.
- [ ] **DoD-5** `decided_by` + `folder_class` + `folder_class_rank` + `source_rank` + `has_bcc` + `date_filetime_utc` + `date_source` appended to the decision CSV with closed vocabularies.
- [ ] **DoD-6** "All Custodians" aggregate (`duplicate_source_count` / `duplicate_sources`, basename, cap 8, truncated flag) present and identical in **decision CSV unique rows, `keep_set_v1` JSON, and `export_messages.csv`**.
- [ ] **DoD-6b** `winners_from_recoverable_items` stat + `--prefer-folder-class` hint, proven signal-only (winners unchanged when it fires).
- [ ] **DoD-7** Opt-in `--fidelity-rank graded` per §3.6 with exhaustive reason→tier mapping test, **or** an explicit **D-0075-graded** deferral recorded in `review.md` with reason.
- [ ] **DoD-8** All three CLI surfaces (`keep-set`, `unique-eml`, `unique-pst`) expose the new flags with consistent names, help text, and error messages; both duplicated policy parsers updated.
- [ ] **DoD-9** Desk wizard offers `earliest_date`, `Prefer folder class`, and `Prefer BCC copy`; wizard arg-mapping unit test updated.
- [ ] **DoD-10** Backward compatibility proven: golden winner set unchanged with default flags; decision CSV header prefix test; pre-0075 `keep_set_v1` JSON still deserializes.
- [ ] **DoD-11** Determinism proven under shuffled input order.
- [ ] **DoD-12** Source PSTs byte-identical before/after every new integration test (SHA-256 full-file, existing pattern).
- [ ] **DoD-13** `docs/unique-pst-export.md` documents the ladder, the policy table, the folder-class ladder with its judgments, the ladder-asymmetry table, glob syntax, the `earliest_date` honesty note, the **sender-copy/BCC** guidance, the **`Versions` items may be structurally altered** warning, and **`first_seen` = sorted path order** with the INC example.
- [ ] **DoD-14** Test suite §3.11 items 1–11 present and passing; synthetic fixtures only.
- [ ] **DoD-15** Full gate green: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`.
- [ ] **DoD-16** `review.md` written with residuals appended to `docs/deferred.md` as **D-0075-***; `conductor.md` + `sequencing.md` flipped to **Completed**; ledger transaction committed.

---

## 8. Verification commands (reference)

```powershell
# Targeted during work
cargo test -p dedup-engine keepset
cargo test -p pst-dedup-cli --test keep_set
cargo check -p pst-dedup-gui

# Surface smoke (synthetic fixture; no client data)
cargo run -p pst-dedup-cli --release -- keep-set fixtures\aspose_outlook.pst `
  --policy earliest_date --prefer-folder-class `
  --decision-csv output\dec.csv --keep-set-json output\ks.json --json

# Full gate before commit
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
ledgerful verify
```

Remove anything written under `output\` before finishing.

---

## 9. Handoff

**Do:**

- Land the `RankContext` refactor as its own no-behavior-change step, then add rungs one at a time.
- Keep folder classification a pure string function in `dedup-engine` — it must be unit-testable without a PST.
- Name every rung in `decided_by` the moment you add it.

**Do not:**

- Change default winners. Any diff in the golden regression without a flag is a bug, not a "better" answer — including "obviously correct" demotions like `Recoverable Items/Versions` (declined on review; ship the stat instead).
- Use raw substring matching for the built-in folder ladder, or accept regex in `--folder-rank` (segment globs only).
- Fabricate BCC from recipient tables, folder names, or sender identity — absent means absent.
- Ship the "All Custodians" aggregate to JSON only, or put absolute client paths in it.
- Invent a date from file mtime, `PidTagLastModificationTime`, or wall clock.
- Reorder or reinterpret existing CSV columns.
- Extend grouping semantics (custodial scope, tier changes) — that is D-0075-scope / 0076.
- Start before 0074 merges, or you will hand-merge `scan.rs` and `unique_pst_cmd.rs`.

**Rollback:** every rung is behind a flag defaulting to today's behavior; reverting the CLI flag registration restores pre-0075 output while leaving the engine additions inert.
