# Unique PST export (`pst-dedup unique-pst`)

Headless operator path (Series K / track **0071**): multi-input PSTs → keep-set winners → streaming unique PST volume(s) → defensible report pack + verification.

> **Operator eDiscovery runbook (0081):** collection → process → handoff → disposition, exit codes, integrity thresholds, ScanPST-on-copy count-diff, and basename custody —  
> [`unique-pst-ediscovery-runbook.md`](unique-pst-ediscovery-runbook.md). This page remains the **flag encyclopedia**.

## One-liner

```powershell
.\target\release\pst-dedup.exe unique-pst a.pst b.pst `
  --out C:\export\unique.pst `
  --report-dir C:\export\unique_report `
  --policy first_seen `
  --max-volume-bytes 10737418240 `
  --json
```

## Pipeline (no re-dedupe)

1. **Integrity scan** (same modes/thresholds as `scan` / `unique-eml`)
2. **`keep_set_v1` resolve** + **`finalize_with_materialize`** (hard promote; optional Mode A attach-incomplete promote when `--promote-on-attach-fail`)
3. **Streaming write** via `write_unicode_pst_streaming` (attachments streamed; never re-dedupe)
4. **Report pack** under `--report-dir`
5. **Verify** each completed volume (open + count + sample MID; optional `--verify-hash`)
6. **QC** (0080) — source-differential reader checks at `--qc-level` (default **sample**)

Source PSTs are **read-only**. The writer never mutates inputs.

## Folder tree contract (0095)

Preserve-layout unique-PST trees are counsel-useful and QC-honest:

| Rule | Behavior |
|---|---|
| **Leading IPM/root aliases** | Strip a **consecutive leading** run of case-folded sentinels only: `root`, `top of personal folders`, `top of information store`, `top of outlook data file`, `ipm_subtree`. Stop at the first non-alias segment. Never strip a later user folder that happens to match (e.g. `Inbox/Top of Personal Folders` stays). |
| **Multi-source prefix** | With `--folder-layout preserve` (default), file-stem prefixes are applied when ≥2 distinct winner sources are known. unique-pst **pre-seeds** the full winner `source_path` set so prefixes are stable from message 1 (closes D-0070). |
| **Unique Mail (residual)** | In preserve mode, `Unique Mail` is allocated **lazily** on the first residual / unparseable path. Fully preserved trees have **no** empty Unique Mail ghost. Flat layout still creates the display-name folder up front. |
| **Flat isolation** | `--folder-layout flat` routes all mail under `Top of Personal Folders/<display-name>` and is unchanged by alias strip / prefix pre-seed. |
| **QC keys** | `folder_tree_structure` expected and output keys use the same alias strip + segment sanitize as the writer (quotes/`*`/trailing dots). Message-bearing **Deleted Items** is claimable (not treated as a system slot). |

**Migration:** scripts that assumed a doubled `Top of Personal Folders` under IPM, or an always-present empty `Unique Mail` folder on preserve exports, must update path expectations.

## Flags

| Flag | Notes |
|---|---|
| `--out <path>` | **Required** — primary PST (volume 1) |
| `--report-dir <dir>` | Default: sibling of `--out` stem + `_report` (e.g. `unique.pst` → `unique_report`) |
| `--input` / positionals | One or more source PSTs |
| `--qc-level off\|structure\|sample\|full` | **0080** — QC depth (default **`sample`**) |
| `--qc-sample-max <N>` | Risk-weighted sample cap (default **64**) |
| `--qc-external-reader <path>` | BYOB path to `pffinfo` / `readpst` (counts only; never auto-download) |
| `--qc-scanpst` | Attempt local `scanpst.exe -no repair` on a temp copy when discoverable |
| Keep-set / integrity | Same family as `unique-eml` (`--policy`, `--family-policy`, `--mode`, thresholds, …) |
| `--policy earliest_date` | Prefer earliest submit time (delivery fallback; missing last). See [Winner policies](#winner-policies). |
| `--prefer-bcc-copy` | Prefer copy with non-empty PidTagDisplayBcc (sender-copy completeness) |
| `--prefer-folder-class` | Built-in folder ladder (Sent Items > live > … > Recoverable Items) |
| `--folder-rank <PATTERN>` | Custom folder demotion ladder (repeatable, worst-last; segment globs; replaces built-in) |
| `--source-rank <SUBSTRING>` | Ordered source preference (repeatable, best-first; unmatched worst) |
| `--rank-folder-class-first` | Swap source_rank ↔ folder_class rungs |
| `--fidelity-rank binary\|graded` | Binary (default, pre-0075) or multi-tier graded fidelity |
| `--folder-layout` | `preserve` (default) or `flat` — see [Folder tree contract](#folder-tree-contract-0095) |
| `--max-volume-bytes` | Soft physical-size ceiling; **off** = single volume |
| `--overwrite` | Required to replace existing `--out` / non-empty report-dir |
| `--verify-hash` | Full-file rehash vs report digests (default **off** for multi-GB comfort) |
| `--also-eml <dir>` | **0107** — co-export a unique-eml pack from the **same** keep-set (no second scan). Directory required; `--overwrite` replaces a non-empty dir. Nested MIME follows `--max-embedded-depth`. BCC on EML still follows unique-eml policy (`Bcc:` from `display_bcc` when present); `--include-bcc-recipients` does not change EML headers. |
| `--attach-ledger full\|summary-only\|off` | **0073** — attachment failure ledger (default **`full`**) |
| `--attach-ledger-max-rows <N>` | Cap on `export_attachments.csv` rows (default **500000**); histogram never truncated |
| `--ledger-path-mode full\|basename` | **0081** — how `source_path` is written in export CSVs (default **`full`**). Basename is handoff-only; join via `source_id` + Matter Archive (see [runbook](unique-pst-ediscovery-runbook.md) §7) |
| `--deep-attach-preflight` | **0074** — opt-in budgeted attach stream probe before keep-set resolve (default **off**) |
| `--deep-attach-level head\|full` | Probe depth: **`head`** (L2, default) or **`full`** (L3). L2 ≠ full verify. Unknown level is a usage error. Under **`--mode strict`**, probe fails **skip** the message (same as attach-meta/body strict); best-effort **degrades** only. |
| `--deep-attach-max-attaches` | Hard stop on attach count probed (default **50000**) |
| `--deep-attach-max-probe-bytes` | Global I/O budget (default **256 MiB**) |
| `--deep-attach-per-attach-max-bytes` | L2 head-read cap per attach (default **1 MiB**) |
| `--deep-attach-max-probe-time-ms` | Per-attach wall-clock abort (default **2000**) |
| `--deep-attach-max-open-psts` | Bounded sticky PST handle LRU (default **32**) — avoids FD exhaustion |
| `--deep-attach-max-peer-probes` | Max peers probed per keep-set group (default **3**) |
| `--max-attach-fail-rate` | Preflight escalate when attach fail rate exceeds (default **0.05**) |
| `--json` | Summary JSON on **stdout**; human progress on **stderr** |
| `--max-open-psts <N>` | **0079** — max sticky source PST handles for materialize + attach stream (default **32**, LRU) |
| `--include-bcc-recipients` | **0082** — write Bcc TC rows + `PidTagDisplayBcc` into the unique-PST (default **OFF**). Default suppresses BCC on the deliverable so consolidating custodians does not over-disclose relative to a single custodian's outward view. **Identity hashing still includes BCC** when the source recipient table is present (internal only). See [BCC disclosure](#bcc-disclosure-0082). |
| `--promote-on-attach-fail` | **0083 Mode A** — pre-write promote when a keep-set peer materializes with incomplete attaches and a ranked peer is complete (default **off** = Mode C ledger-only). Mode B write-time mid-message promote is **not** supported. Under default global scope may perform **cross-custodian de-duplication** — see [runbook](unique-pst-ediscovery-runbook.md). Pair with `--deep-attach-preflight` for richer incomplete detection. |
| `--max-embedded-depth <1-8>` | **0101** — nested `ATTACH_EMBEDDED_MSG` extract/write depth (default **3**). Values outside **1–8** are a usage error (not silently clamped). Deeper nests ledger `ATTACH_DEPTH_LIMIT`. The 32 MiB per-nest byte budget also maps to that same code. |

### Deep attach preflight (0074) — honesty

| Claim allowed | Claim forbidden |
|---|---|
| “Budgeted L2 head-probe of N attaches; M failed; rate R” | “All attachments will export cleanly” |
| `recommendation: re_export_recommended` from attach rate | Silent `ok` when rate exceeds threshold |
| Residual mid-tail risk after L2 | Equating L2 success with L3 full verification |

- **Default off** on both `scan` and `unique-pst` (opt-in). Skipped entirely under `parents_only` / `--no-attachments`.
- Probe fails **degrade** message integrity so `fidelity_rank` prefers clean peers; does not invent a new exit code.
- When attach fail rate > `--max-attach-fail-rate`, preflight escalates `ok` → `re_export_recommended` + reason `attach_stream_fail_rate_exceeded`.
- **0073 residual ledger** at export is still required for mid-tail / residual fails.
- **0077** owns CRC stderr noise — probe uses structured counters only (no per-page CRC log spam).
- **Materialize does not re-probe** when phase-1b deep preflight already ran (shared hard budgets). Winner fidelity is from phase-1b degrade; residual mid-tail fails → 0073 ledger.
- **Cancel** during probe sets `attach_probe.cancelled=true` and marks coverage incomplete (`truncated` + coverage note).
- Plain **`pst-dedup scan`** accepts the same deep-attach budget flags, including `--deep-attach-max-peer-probes` (default **3**).
- High rates → **re-export from Purview/Exchange** preferred. **ScanPST only on a copy**, last resort (may change metadata). Never auto-ScanPST; never mutate sources.
- Preflight JSON nests `attach_probe` under `summary.preflight` (always present; `enabled: false` when off).

## Multi-volume naming

| Volume | Path |
|---|---|
| 1 | `--out` (e.g. `C:\export\unique.pst`) |
| 2+ | `{stem}_vol002.pst`, `{stem}_vol003.pst`, … next to `--out` |

Split is **between messages only** (after a full keep-set winner family is written). Progress sink uses **physical** temp size (`current_physical_size`), not payload-sum alone.

### Store RecordKey & re-run reproducibility (0087)

| Claim | Status |
|---|---|
| **Logical store identity** (`PidTagRecordKey` / EntryID ProviderUID) | **Hard guarantee** under default deterministic mode |
| Full volume-file `sha256_hex` / `md5_hex` match across re-runs | **Best-effort** — subject to B-tree / page layout stability |
| Dest path in preimage | **Never** — same winners → same RecordKey on different `--out` paths |

Default mode is **deterministic**. Summary JSON reports `store_record_key_mode: "deterministic"`. Preimage is domain-separated SHA-256 (algo v1) over `volume_index` + message count + content fingerprint (length-prefixed MID / subject / submit time / folder path per message in write order). unique-pst also passes a **job-global** seed (`store_key_material`) from ordered keep-set winner loci so multi-volume keys bind to the whole job.

**Volume-layout coupling (not a bug):** changing `--max-volume-bytes` (or any policy that changes which messages land on which volume) **breaks** per-volume RecordKey and volume-digest reproducibility even when the global winner set is identical. Re-run stability requires **identical chunking layout**, not only identical winners.

**RecordKey vs volume-hash:** RecordKey seals **logical store identity**. Volume digests remain useful custody seals when layout is stable; when they drift, use the **0079 structural equivalence oracle** (`export_oracle::compare_export_packs`) — content/structure parity still holds.

**Optional 0086 synergy (docs only, not mandated):** operators already paying for `--strong-content-hash body-recip-attach` may pass an aggregate strong-content / keep-set fingerprint as `store_key_material` so the store key is attachment-byte-aware. Default volume-local fingerprint remains **metadata-only** (MID/subject/time/folder) so unique-pst never forces attach I/O solely for store keys.

### Oversized family vs soft limit

A single winner (parent + attaches) may **exceed** `--max-volume-bytes` by itself. The export **allows the exceed** rather than severing the family or failing the run. The volume row may set `volume_exceeded_soft_limit: true`.

## Partial failure (mid-volume)

If volume *k* fails fatally (disk full, path unwritable, layout hard fail):

1. **Completed** volumes `1..k−1` are **retained** (openable PSTs).
2. The **in-progress** volume (temp or incomplete final) is **deleted**.
3. Report pack still flushes with `ok: false`, `export.partial: true`, and only completed volumes listed.
4. Process exits **non-zero**.

## Report pack

```text
{report-dir}/
  summary.json              # unique_export_report_v1 (+ 0073 attach histogram fields; 0101 always-present export.max_embedded_depth — schema id not bumped)
  decisions.csv             # keep-set decision stream
  keepset.json              # winners + stats (no bodies)
  volumes.csv               # one row per completed volume (+ sha256/md5)
  export_messages.csv       # MANDATORY winner → volume cross-reference
  export_body_cloud_links.csv  # 0085 body-inline document-shaped cloud URL hit-list
  qc_report_v1.json         # 0080 QC summary (when --qc-level ≠ off)
  qc_findings.csv           # 0080 row-level findings
  content_digests.json      # 0080 source-side digests for clean-room re-verify
  qc_attestation_v1.json    # optional human-signed operator attestation (never auto-written)
  export_attachments.csv    # 0073 attach failure ledger (mode=full)
  integrity.csv             # optional / if requested
```

### `summary.json` phase timings (0079)

Additive fields (always present; older tools may ignore):

| Field | Meaning |
|---|---|
| `phase_timings.scan_ms` | Integrity scan |
| `phase_timings.deep_attach_preflight_ms` | Opt-in deep attach probe (0 when off) |
| `phase_timings.resolve_ms` | Keep-set resolve / group bind |
| `phase_timings.materialize_ms` | Winner materialize (+ promote) |
| `phase_timings.prepare_ms` | Assemble prepared winners (no second materialize) |
| `phase_timings.write_ms` | Streaming PST write (includes final-hash wall inside writer) |
| `phase_timings.report_ms` | Report pack flush |
| `phase_timings.verify_ms` | Phase 5 volume verify |
| `phase_timings.qc_ms` | Source-differential / external QC (0080/0130). 0 when `--qc-level off` |
| `phase_timings.also_eml_ms` | Co-export `--also-eml` wall (0129). **0** when the flag is omitted |
| `phase_timings.quarantine_ms` | Cancel quarantine rename (0 when not cancelled) |
| `phase_timings.unaccounted_ms` | `total_ms − Σ(phases)` — **computed, never fudged to 0** |
| `phase_timings.total_ms` | Wall from orchestration start |
| `source_pst_opens` | Successful `PstFile::open` count via shared LRU cache |
| `messages_materialized` | Must equal `keep_set.stats.unique` (single materialize) |
| `bytes_written_total` | Sum of completed volume sizes |
| `prepared_bytes_peak` | Peak retained body + buffered-attach bytes in `prepared` |
| `hash_ms` | Final-hash (SHA-256+MD5) wall across volumes |
| `store_record_key_mode` | **0087** — `"deterministic"` (default) or `"ephemeral"` |

Soft warning when `prepared_bytes_peak` exceeds **1 GiB** (stability; see D-0079-stream-prepare).

**Oracle allowlist:** structural pack compare (`export_oracle`) strips the additive
measurement fields above (plus paths/hashes/timings) so a **pre-0079 parent** pack
without them still compares equal to HEAD on **measurement** product semantics.
That equalization does **not** cover `export_risk.inputs` attest fields (0099):
a **pre-0099** parent that omits `effective_block_crc_read_rate` /
`poly_class_crc_discounted` / `discount_attach_stream_crc` /
`poly_class_crc_sources` **must mismatch** HEAD — intended (that is the attest).
Job-level `summary.inputs` (source paths) is blanked at **root only**; do **not**
recursive-strip the name `inputs` — that object key is also `export_risk.inputs`
(product attest). Oracle pointers `/export_risk/inputs/effective_block_crc_read_rate`
(and siblings) must compare. Operator parent-vs-HEAD gate: build a **post-0099**
parent binary, set `PST_DEDUPE_BASELINE_BIN`, run
`unique_pst_parent_baseline_oracle_when_env_set` (or call `compare_export_packs`);
a pre-0099 baseline is expected-red on those pointers.

### Sensitivity (handoff)

The entire **`report-dir` is operator-sensitive**: absolute paths, folder names, subjects, and attachment filenames can leak PII or privilege context. Do **not** post report packs to untrusted third parties without redaction. Prefer sharing the summary histogram + reason codes first. Primary join key that avoids path strings: **`source_id`** (0-based index into `summary.inputs`). Optional basename redaction: `--ledger-path-mode basename` (**closed D-0073-basename** in 0081; default remains `full` — see [runbook](unique-pst-ediscovery-runbook.md) §7).

### CSV open safety

Free-text cells in `export_attachments.csv`, `export_messages.csv`, and `export_body_cloud_links.csv` neutralize spreadsheet formula injection: values whose leading non-space character is `=`, `+`, `-`, or `@` are prefixed with `'` before CSV quoting (**without** rewriting URL structure). Still open CSVs as text when possible; treat the pack as sensitive.

### `export_messages.csv` (mandatory)

Fixed columns (prefix locked; **0073**/**0075**/**0081**/**0082**/**0085** append only):

```text
source_path,folder_path,nid,message_id_norm,edrm_mih,content_hash_hex,volume_path,volume_index,export_message_index,attachments_failed_count,duplicate_source_count,duplicate_sources,source_id,bcc_suppressed,body_cloud_link_count
```

One row per **successfully written** unique winner. **No body text** columns.

`attachments_failed_count` is the number of **fail-severity** attach outcomes for that message on the write path (left-join alternative: count fail rows in `export_attachments.csv` for the same `source_path` + `msg_nid`).

`duplicate_source_count` / `duplicate_sources` (**0075**): “All Custodians” aggregate — distinct **source PST basenames** (not absolute paths) that held a suppressed copy of this winner, `|`-delimited, capped at 8 (same values as decision CSV unique rows and `keep_set_v1` JSON).

`source_id` (**0081**): 0-based index into `summary.inputs` (decimal string; empty when unmapped — never invented as `0`). Join key under `--ledger-path-mode basename` when multiple inputs share a basename.

`bcc_suppressed` (**0082**): `true` when the source had one or more Bcc recipients (recipient-table Bcc row **or** non-empty `display_bcc`) **and** the write path omitted them (default; `--include-bcc-recipients` not set). `false` when BCC was written, or the source had no BCC.

## Winner policies

Keep-set chooses one exportable copy per identity group. With **no** 0075 flags, winners match pre-0075 behavior.

### Rank ladder (lower is better)

```text
fidelity → bcc_completeness → source_rank → folder_class → policy → path_key → nid
```

- New rungs are **constant 0** when their flags are off (zero silent change).
- `--source-rank` outranks `folder_class` by default (explicit custodian/file preference beats a folder heuristic).
- `--rank-folder-class-first` swaps only those two adjacent rungs.
- Every comparison terminates in `(path_key, nid)` for determinism.

### Policies (`--policy`)

| Policy | Meaning |
|---|---|
| `first_seen` (**default**) | **Sorted input-path order**, then scan index — **not** chronological send time. Matches Relativity’s “first published copy is master”. |
| `keep_largest` | Prefer largest message size |
| `prefer_path` | Prefer paths matching `--prefer-path-contains` (unordered boolean) |
| `earliest_date` | Prefer earliest **submit** FILETIME; **delivery** only if submit missing on that item; missing/≤0 sorts **last** |

**Honesty — `first_seen`:** if `INC0102784-2.pst` sorts before `INC0102784.pst` by absolute path, the `-2` file can crown winners even when the primary file is preferred operationally. Remedy: `--source-rank INC0102784.pst --source-rank INC0102784-2.pst`.

**Honesty — `earliest_date`:** duplicate copies usually share the same sent time, so this policy often ties and falls through to path order. Real effects: demoting undated dumpster/Versions copies, and Tier-1 groups whose members genuinely differ. **Tier-2 groups cannot differ on submit time** (submit is already in the content hash) — `earliest_date` is a no-op inside a pure Tier-2 group. Never invent dates from mtime / LastModificationTime / wall clock.

### Sender-copy / BCC (`--prefer-bcc-copy`)

In a Message-ID group, the sender’s Sent Items copy is often the only one carrying BCC (stripped in transport). Enable **`--prefer-bcc-copy`** with **`--prefer-folder-class`** together when BCC evidence matters. Empty/whitespace BCC counts as absent (never fabricated). Stats always include `winners_without_bcc_peer_had_bcc` so silent BCC loss is visible even when the flag is off.

### Folder-class ladder (`--prefer-folder-class`)

Whole-segment, case-insensitive matching. Recoverable Items children are **parent-qualified** (`Purges` alone does not demote a user folder named Purges).

| Rank | Class | Matches |
|---|---|---|
| 0 | `sent_items` | Sent Items, Sent Mail |
| 1 | `primary` | Inbox / user folders (default when ladder off) |
| 2 | `archive` | Archive, Online Archive, In-Place Archive* |
| 3 | `junk_email` | Junk Email, Junk E-mail, Spam |
| 4 | `drafts` | Drafts |
| 5 | `outbox` | Outbox |
| 6 | `deleted_items` | Deleted Items |
| 7–12 | `recoverable_*` | Under Recoverable Items: Deletions, holds, Purges, Versions, ops, other |

**Judgments:** sent_items above primary (richest evidence); drafts/outbox below junk (non-transmitted); recoverable_versions **below** purges (copy-on-write **modified** originals may be structurally altered — subject/body/attachments/participants/dates — yet still tie on submit_time; the folder ladder separates them, not `earliest_date`).

When any winner is from Recoverable Items, the run reports `winners_from_recoverable_items` and a human hint recommending `--prefer-folder-class` — **signal only** (winners unchanged unless the flag is on).

**Custom `--folder-rank`:** ordered **worst-last**; rank = `1 + first match index`; unmatched = **best** (0). Segment globs: `*` at start and/or end of a segment only (`*Purges`, `*Element*`). No regex. Supplying any `--folder-rank` **replaces** the built-in ladder.

### Source rank (`--source-rank`)

Ordered **best-first** substrings against the absolute source path (Windows lowercased). Unmatched = **worst** (`patterns.len()`). Documented asymmetry vs folder-rank:

| Flag | Enumerates | Unmatched |
|---|---|---|
| `--folder-rank` | Bad places | Best |
| `--source-rank` | Preferred files/custodians | Worst |

### Graded fidelity (`--fidelity-rank graded`)

Default remains **binary** (clean=0, any degraded/orphaned=1). Graded uses worst tier across integrity reasons: soft attach meta (1) < attach payload (2) < body (3) < structural (4). `CRC_SUSPECT` is graded tier 3 (body/data class) so a clean twin outranks a suspect copy.

### CRC integrity & export risk (0077)

Scan and unique-pst report page/block CRC and BID mismatch counters **per source** (data path — not dependent on a log subscriber). CRC remains **warning-only and non-fatal**; `crc_skip_rate` still means message-level CRC *skips* only.

| Signal | Read it as | Action |
|---|---|---|
| `distinct_bad_bids` small, `page_crc_mismatches` huge | a few bad blocks re-read many times | usually proceed; check attach fail rate |
| `distinct_bad_bids` large / `exact=false` | widespread block corruption **unless** `poly_class_crc` is true (every block looks “bad” on a non-standard CRC store) | re-export only when not poly-class; on poly-class this is expected |
| `crc_suspect_messages` > 0 | documents **kept with possibly-wrong bytes**; held out of Tier 2 by default | higher unique count expected; flagged, not lost |
| `block_crc_read_rate` ≥ 0.15 | the medium is failing, not the mailbox — **unless** that rate is poly-class noise excluded via `effective_block_crc_read_rate` | `export_risk = not_export_ready` when the **effective** (non-poly) rate crosses 0.15 |
| `poly_class_crc` / `poly_class_crc_discounted` | computed≠stored is the store’s CRC (aspose / Permute-class), not a bad image | raw counters stay on `inputs` for the affidavit; post-export CRC gates do not elevate |
| `ATTACH_STREAM_CRC` Info on a poly-class-only job | same trailer mismatch as pages/blocks | does **not** elevate `export_risk` after 0099 (`discount_attach_stream_crc`) |
| Keep-set `CRC_SUSPECT` on poly-class winners | materialize re-taints winners even when scan cleared poly false-positives | alone does **not** elevate `export_risk` after 0108 when `effective_degraded_winner_rate` is used; raw `degraded_winner_rate` stays on `inputs`; body/attach degrade still keys the effective rate (`max_degraded_winner_rate=0.02` unchanged) |
| `attach_fail_rate` over threshold | attachment payloads unreadable | re-export; the 0073 ledger names which |
| `export_risk = not_export_ready` | failed volume, catastrophic **effective** rate, attach fail, or scan already said so | do not hand off |

`poly_class_crc_discounted` **may co-occur** with a non-CRC `not_export_ready` reason (`scan_recommendation=not_export_ready`, failed volume, attach fail). Both are true; `level` is still `max(scan, post)`. The Desk wizard banners on `level`.

**CLI:** `--crc-log-limit` (default 10), `--crc-log-interval-secs` (default 30), `--allow-crc-suspect-tier2` (default off).

**`export_risk`** reuses `PreflightRecommendation` (`ok` | `re_export_recommended` | `not_export_ready`) and is the **max** of scan preflight and post-export evaluation (export never lowers risk). Exit-code mapping is **0078**.

#### ScanPST

- **`SCANPST.EXE` modifies the file it repairs** (writes a `.bak` first — Microsoft documentation). Run it on a **copy**, never on operator evidence. Repairing evidence in place is a chain-of-custody event.
- **ScanPST repairs by discarding what it cannot recover.** "Repair complete" means structural consistency, not "nothing was lost."
- **Always diff the counts:** `pst-dedup scan <original> --json` before, `pst-dedup scan <repaired-copy> --json` after; compare `total_messages` and per-folder counts. Log any drop as disclosed data loss with the delta stated.
- ScanPST ships with **classic** Outlook (Microsoft 365 / 2024 / 2021 / 2019 / 2016). Do not assume it exists on a "new Outlook" machine.

#### Purview

- Before concluding a Purview export is corrupt: re-export with *"Also include items that have an unrecognized format, are encrypted, or weren't indexed"* and read the **unindexed items report**.
- **Unindexed ≠ corrupted.** CRC block errors are *physical* byte corruption (remedy: re-download / re-export). Purview unindexed items are *logical* indexing exceptions in a byte-perfect file — password-protected, unsupported format, oversized (remedy: decrypt, different extractor, or documented exclusion). Wrong remedy wastes days.
- Purview PSTs have been reported opening empty or without `Top of Information Store` at a correct byte size — **check folder and message counts, not file size**.

#### This tool

- Never repairs a source. Remediation is re-export or a repaired **copy** (new evidence item with its own count delta).

Cross-links: **0078** exit codes from `export_risk`; **0081** operator runbook.

### Explainability columns

Decision CSV appends (end of row): `folder_class`, `folder_class_rank`, `source_rank`, `has_bcc`, `date_filetime_utc`, `date_source`, `decided_by`, `duplicate_source_count`, `duplicate_sources`.

**`decided_by` vocabulary:** `sole_member`, `fidelity`, `bcc_completeness`, `source_rank`, `folder_class`, `policy_first_seen`, `policy_keep_largest`, `policy_prefer_path`, `policy_earliest_date`, `path_order`, `nid`, plus promote strings (0083):

| Token | Meaning |
|---|---|
| `promoted_after_materialize_fail` | Hard materialize fail on earlier peer(s); later peer accepted |
| `promoted_after_attach_incomplete` | Mode A: incomplete attach on earlier peer(s); later **complete** peer accepted |
| `mode_c_fallback_all_peers_incomplete` | Mode A flag on; every materializable peer was attach-incomplete; exported highest-ranked materializable |

Filter `mode_c_fallback_all_peers_incomplete` when Mode A could not recover a complete copy.

**`date_source`:** `submit` | `delivery` | `none`.

Cross-links: **0080** QC sampling by `folder_class`; **0081** operator runbook (which policy for which collection).

### `export_attachments.csv` (0073; `--attach-ledger=full`)

Streamed / batched via a background writer thread (critical path enqueues only; no fsync-per-row). One row per attach outcome of interest — **never** one row per CRC page.

```text
source_id,source_path,folder_path,msg_nid,attach_nid,attach_index,filename,size,attach_method,reason_code,severity,volume_path,volume_index,winner_promoted,peer_source_id,peer_msg_nid,message_subject
```

| Column | Notes |
|---|---|
| `source_id` | 0-based index into `summary.inputs` (same order as CLI inputs) |
| `source_path` | **Same string** as `export_messages.csv` for join |
| `severity` | `fail` increments `attachments_failed`; `info` does not (policy omit, truncation marker) |
| `reason_code` | Stable `SCREAMING_SNAKE` — see reason→action below |
| `winner_promoted` | `true` when Mode A (or hard materialize promote) selected a later peer: soft-skip incomplete rows and write-time fails on promoted winners (0083) |
| `peer_source_id` / `peer_msg_nid` | Final accepted winner locus when `winner_promoted` is set on a soft-skipped incomplete peer |

**Modes:**

| `--attach-ledger` | CSV | Histogram in summary |
|---|---|---|
| `full` (default) | Yes | Yes |
| `summary-only` | No | Yes |
| `off` | No | No (counts still honest for exit) |

**Row cap:** default 500 000 CSV rows; when hit, one final `ATTACH_LEDGER_TRUNCATED` (`severity=info`) is written, CSV stops, histogram + `attachments_failed` continue, `attachment_ledger_truncated=true`.

### summary.json attach fields (additive v1)

```json
"export": {
  "attachments_written": 18609,
  "attachments_failed": 366,
  "attachments_omitted_by_policy": 0,
  "attachments_failed_by_reason": { "ATTACH_STREAM_READ_FAILED": 200 },
  "attachment_ledger": "export_attachments.csv",
  "attachment_ledger_mode": "full",
  "attachment_ledger_truncated": false,
  "attachment_ledger_rows_written": 366
}
```

### `preflight.attach_probe` (0074; nested under scan preflight)

```json
"attach_probe": {
  "enabled": true,
  "level": "head",
  "attempted": 12000,
  "failed": 80,
  "truncated": false,
  "fail_rate": 0.0067,
  "max_attach_fail_rate": 0.05,
  "coverage_note": "budgeted attach probe level=head; residual export ledger (0073); L2 ≠ full verify",
  "peer_probe_capped_groups": 0
}
```

Also available on plain `pst-dedup scan --deep-attach-preflight --json` under `summary.preflight.attach_probe` (same budget flags as unique-pst: level, max-attaches, max-probe-bytes, per-attach-max-bytes, max-probe-time-ms, max-open-psts, **max-peer-probes**, max-attach-fail-rate).

### Reason → operator action (buckets)

| Bucket | Example codes | Operator action |
|---|---|---|
| Corrupt / unreadable | `ATTACH_STREAM_CRC`, `ATTACH_BLOCK_NOT_FOUND`, `ATTACH_DATA_TRUNCATED`, `ATTACH_STREAM_OPEN_FAILED`, `ATTACH_STREAM_READ_FAILED` | Re-export source; ScanPST (external); do **not** claim in-tool repair |
| Non-portable method | `ATTACH_METHOD_UNSUPPORTED` | EML path, re-export with cloud content, or accept omit |
| Fidelity limit | `ATTACH_DEPTH_LIMIT`, `ATTACH_EMBEDDED_UNPARSED` | Residual fidelity; peer promote if available (not auto in P0) |
| Policy | `ATTACH_OMITTED_BY_POLICY` | Expected under `parents_only` — not a fail |

### Default hash trust vs `--verify-hash`

- **Default:** report digests come from the writer (`WritePstReport`); Phase 5 does **not** re-read multi-GB files solely to rehash.
- **Structural proof:** open with `pst-reader`, message count == `messages_written`, sample ≥ min(5, N) Message-IDs.
- **`--verify-hash`:** independent full-file SHA-256; sets `verification.hash_match` (use on small fixtures / CI).

## Identity and binding (0076)

Dedup identity is tiered. Defaults may only **split** groups relative to pre-0076 (never merge more).

| Tier | Key | Default |
|---|---|---|
| **1** | Normalized `InternetMessageId` | Always on; MID match is definitive |
| **2** (v1) | SHA-256 of normalized subject \| submit FILETIME \| sender \| ≤4096 **chars** body preview \| sorted attach `name:size` | On (`--no-tier2` disables) |
| **2.5** (v2) | v1 preimage **plus** layered extras | Off — `--strong-content-hash body\|body-recip\|body-recip-attach` (**0086** ships attach-content; default remains `off`) |

**Named divergences from Relativity’s four-component hash:**

| Component | Relativity | This tool (v1 default) |
|---|---|---|
| Body | Full `PR_BODY` with CR/LF/space/tab stripped | First **4096 characters** of whitespace-normalized body (spaces kept) |
| Header | Subject + sender name/email + ClientSubmitTime | Subject + sender email + submit FILETIME (no separate display name) |
| Recipients | All recipients incl. BCC (address-oriented) | **Absent at v1**; opt-in at `body-recip` via structured table keys when present, else display strings (**0082**) |
| Attachments | Per-attachment **content** SHA-256 (“normal standard SHA256 file hash”; **no published block size** — our stream chunk is an implementation detail) | Name + size at v1 / `body` / `body-recip`; full-stream content digests at **`body-recip-attach` (0086)** |

### Attach-content identity (`body-recip-attach`, 0086)

Opt-in only (expensive full-stream attach I/O). When set, every non-ignored attachment contributes a **32-byte slot** to the strong preimage:

| Condition | Slot | Stats |
|---|---|---|
| By-value stream fully read; `bytes_read` matches declared size when size &gt; 0 | Real `SHA-256(stream_bytes)` | `strong_hash_attach_digested` / `_bytes` |
| Declared size **0**, open succeeds, immediate EOF | Real `SHA-256("")` = `e3b0c442…` (legitimate empty file) | digested |
| Length mismatch (size &gt; 0 but stream empty/short), open/CRC/IO fail, cancel, budget | **Choice B unread sentinel** (never omit; never tier-downgrade to `body-recip`) | `strong_hash_attach_unread` |
| Cloud-link / no offline binary (`is_cloud_link`) | Unread sentinel (no open attempt) | unread |

**Choice B unread sentinel (exact formula):**

```text
SHA-256( b"pst-dedup/attach-unread/v1\0" || name_lower_utf8 || b"\0" || size as little-endian u32 )
```

So unread `Contract.pdf` ≠ unread `Financials.xlsx` ≠ empty-file digest ≠ real content. Slots are **sorted** before folding (order-independent). **Forbidden:** omit missing digests; downgrade incomplete items to `body-recip` binding; use a static single `UNREAD` constant for all failures.

**Budgets (dedicated flags; not 0074 L2 head caps):**

| Flag | Default |
|---|---|
| `--strong-hash-attach-max-attaches` | 50_000 |
| `--strong-hash-attach-max-bytes` | 1 GiB |
| `--strong-hash-attach-per-attach-max-bytes` | 512 MiB |

Identity always **full-streams** under these caps; 0074 deep-attach head probe is unrelated and must not be treated as digest equality.

**`--identity-ignore-inline-attachments` + `body-recip-attach`:** soft **stderr warning** (not hard reject). Inline attaches are omitted from both name:size and content slots — softens the byte-strict promise (logo/signature variance still filterable).

**Mode A interaction:** attach-content identity can place a complete physical copy and an incomplete/cloud copy into **different keep-set groups**. Mode A only promotes **within** a group — it is not a substitute for attach-content identity when attach-byte fidelity matters for grouping.

**Embedded message attaches (`embedded-msg-hash/v1`, 0090):** under `body-recip-attach`, method-5 (`ATTACH_EMBEDDED_MSG`) and by-value `message/rfc822` attaches contribute a **documented pst-dedup nested identity digest** — not unread-sentinel-only (method 5) and not raw-blob-only (rfc822). Preimage:

```text
SHA-256( b"pst-dedup/embedded-msg-hash/v1\0"
  || depth_u8
  || header_hash_32      // norm subject | submit_time | sender
  || body_hash_32        // hash_full_body when body present (incl. empty); missing → embedded-body-missing/v1
  || recipients_hash_32  // SHA-256 of Tier-2.5 recipient preimage
  || attachments_hash_32 // child digests in **attach table index order**, each + ';'
)
```

Missing nested body uses the domain-separated `embedded-body-missing/v1` sentinel component (not the empty-body `hash_full_body("")` digest). Nested body UTF-8 length is charged against the same per-attach / run byte caps as by-value streams. Child embeds recurse with `depth+1` until `MAX_EMBEDDED_MSG_DEPTH = 3`; at the cap use domain-separated `attach-depth-limit/v1` sentinel (not raw blob, not panic). Unreadable nested objects stay Choice B unread. Stats: `strong_hash_embedded_parsed` / `_depth_limit` / `_unparsed`.

**Not Relativity dedupe parity.** Relativity Server hashes four components separately, extracts embedded emails as **child documents**, and does **not** fold nested email into the parent’s AttachmentHash. Recursive hash-in-parent is a pst-dedup product choice for parent-centric keep-sets (matter extract still models children elsewhere).

**Nested unique-pst export (0094 / 0101):** method-5 winners get a bounded nested `WriteMessage`. **CLI owns the knob:** `unique-pst --max-embedded-depth` (default **3**, valid **1–8**; clap rejects outside that range). The same effective value is passed to `materialize_nested_for_winner` and `WritePstOpts::max_embedded_depth`. 32 MiB per-nest ceiling unchanged. Attach PC writes **`PidTagAttachDataObject` PtypObject** (`0x3701` / `0x000D`) for Outlook-discoverable nests; reader resolves via that property (scan fallback for older output). Child by-value attaches under nests stream via `open_attach_data_from_message_node` (nested NIDs are not in the NBT). **unique-eml (0106)** consumes the same `NestedCanonicalMessage` DTO with the same extract helper and the same `--max-embedded-depth` semantics to write reconstructed nested RFC 5322 MIME (matter/Relativity child-document extract remains under **D-0067-embedded-depth**).

**Depth names (do not conflate):**

| Name | Owner | Role |
|---|---|---|
| `WritePstOpts::max_embedded_depth` / `--max-embedded-depth` | writer + unique-pst CLI | Extract/write knob, clamp [1, 8], default 3 |
| `EmlWriteOpts::max_embedded_depth` / unique-eml `--max-embedded-depth` | eml_pack + unique-eml CLI | Extract/write knob, clamp [1, 8], default 3 (0106) |
| `MAX_EMBEDDED_IDENTITY_DEPTH` | `pst-reader` | 0090 hash recursion, **locked 3** |
| `DEFAULT_MAX_EMBEDDED_DEPTH` | `eml_pack` / `named_prop_map` | Default 3 on those surfaces |

`unique_export_report_v1` gained always-present `export.max_embedded_depth` (consumers should ignore unknown keys; schema id **not** bumped).

### Tier-2.5 recipient identity (0082)

When `--strong-content-hash body-recip` (or higher when available) is on **and** the source message has a **non-empty recipient table**, Tier-2.5 fingerprints recipients from structured TC rows — not display-name strings alone.

**Per-row identity key cascade** (exactly one key per row, then sort + `;`-join over **To+Cc+Bcc**):

1. `PidTagSmtpAddress` if non-empty
2. `PidTagEmailAddress` if address type is SMTP (or the address is SMTP-shaped and type is missing/empty)
3. `PidTagEmailAddress` if address type is **EX** / LegacyExchangeDN (`/O=…`, `/OU=…`, `/CN=…` forms kept; case-folded; **not** dropped to display) — typed `EX` counts even without `/O=`
4. Normalized **display name** only when no structured address key exists

Messages with **no readable recipient table** still use the pre-0082 display-string path (`display_to` / `cc` / `bcc`). The reader **never invents** TC rows from Display* props.

**No distribution-list expansion:** fidelity is to the **PST file**. If the source stored only a DL display name / EX address without expanded members, the unique-PST **replicates that row** and does **not** resolve GAL membership.

### Split-only guards (default on)

1. **Unreadable / degenerate body** — items with `body_incomplete` / `body_unavailable`, or no body and fewer than two weak fields (subject / time / sender / ≥1 attach), do **not** bind on Tier 2. Escape: `--allow-degenerate-tier2`.
2. **Cross-MID** — two items with **different** non-empty Message-IDs never merge on content hash. Escape: `--allow-cross-mid-tier2`.

**Bulk-mail warning:** blocking cross-MID merges inflates unique counts most for newsletters, HR templates, and automated mailers (each dispatch has its own Message-ID). Read `cross_mid_blocked_max_group` in the run summary first. Use `--allow-cross-mid-tier2` only when aggressive bulk culling is intentional and recipient-level evidence is not at issue.

**Recipient warning (`body-recip`):** when a recipient table is present, identity uses the SMTP → EX DN → display cascade above (**shipped in 0082**; closes **D-0076-recipient-table**). Table-less messages still fall back to `display_to` / `cc` / `bcc` *display names*, which can vary between copies (`"Smith, John"` vs `"John Smith"`). Check `tier2_5_splits_recipients_only` and `x500_recipient_items` when diagnosing splits.

### BCC disclosure (0082)

| Surface | Default | Opt-in |
|---|---|---|
| **Write** (deliverable) | To + Cc TC rows only; no Bcc rows / no `PidTagDisplayBcc` | `--include-bcc-recipients` writes Bcc when source had them |
| **Identity** (Tier-2.5 hash) | To+Cc+**Bcc** participate when the table is present | Always on for identity when table present — not a disclosure surface |
| **Audit** | `export_messages.csv` column `bcc_suppressed`; summary `bcc_suppressed_message_count` | — |

**Reviewer note:** two near-identical messages in the unique-PST with `bcc_suppressed=true` are **not** a dedupe failure — BCC variance was kept for identity and omitted from the deliverable by policy. See the [runbook](unique-pst-ediscovery-runbook.md).

### Recipient TC Strategy A (0100)

Per-message recipient tables write **every included row** (To/Cc; BCC still 0082 opt-in). The row matrix is a subnode (`hnidRows` = NID) packed with MS-PST §2.3.4.4 `RowsPerBlock = Floor(8176 / row_width)` (live 15-column schema width **56** → **146** rows per leaf). Recipient-table node data uses a multi-page HN (HNHDR + HNPAGEHDR; HID `hidIndex` 1-based **per page**). Empty tables keep `hnidRows = 0` and `bid_sub = 0`. Recipient-table `bid_sub` SLBLOCK entries are **NID-ascending** (0103): `insert(0)` of the matrix NID is forbidden; `add_subnode_leaf` sorts and fail-closes on duplicate NIDs. Production unique-pst does **not** emit `RECIPIENT_TC_TRUNCATED` for included rows; Display* stay full. Differential QC still treats an **injected** matching truncate event as **`known_gap`**; unexplained To/Cc loss without an event remains a **defect**. Residual: HNBITMAPHDR pages 8/136/264 (`D-0100-hn-bitmap-hdr`, fail closed; attach heaps share the same gate).

### Attachment TC Strategy A (0104)

Per-message attachment tables (`NID 0x671`) write **every successfully written attach** as a row (objects MUST match rows). Same Strategy A layout as recipients: row matrix is a subnode (`hnidRows` = NID) packed with `RowsPerBlock = Floor(8176 / row_width)` (live six-column schema width **25** → **327** rows per leaf); attachment-table node uses multi-page HN; filenames &gt; `MAX_HEAP_VALUE_SIZE` (2048) divert to a cell NID on the table SLBLOCK (NID-ascending emit via 0103). Empty messages **omit** `0x671` (MS-PST optional). Store template `0x671` stays zero-row / single-page. The six columns are the MS-PST Attachment Table Template **MUST** set; extra message-table properties are optional and not added. Fail closed on HN/matrix overflow — no `ATTACH_TC_TRUNCATED` / `known_gap`. Residual: HNBITMAPHDR (`D-0100-hn-bitmap-hdr`).

**Inline attachments:** signature logos can false-split attachment-parity comparisons. `--identity-ignore-inline-attachments` (opt-in, merge-increasing) uses MAPI flags (`PidTagAttachContentId` / rendered-in-body / hidden), not a size threshold.

**Scope:** `--dedupe-scope global` (default) vs `per-source` (custodial / vertical — each source’s winners survive). Under `per-source`, the All Custodians aggregate correctly degenerates (each winner lists one source).

**BCC at ≥ `body-recip`:** a sender copy with BCC and a recipient copy without split at Tier 2.5 unless they share a MID (Tier 1 still binds; use 0075 `--prefer-bcc-copy` for winner choice).

**Tier-1 divergence:** MID groups can hold different content (Purview edited-but-unsent). Stats `tier1_divergent_body` / `_metadata` / `_recipients` always report; human hint fires on **body** only. Opt-in `--tier1-verify content|body` splits.

**Reproducibility:** for non-Latin bodies over ~2048 characters, pre-0076 `content_hash_hex` may not reproduce (byte clamp → char clamp). Counted by `tier2_preview_bytes_over_budget`. Change is split-only.

**Closed vocabularies (decision CSV / JSON):** `bound_by` ∈ {`seed`,`message_id`,`content_hash`,`content_hash_strong`}; `identity_version` ∈ {`v1`,`v2`}; `dedupe_scope` ∈ {`global`,`per-source`}.

**Unique counts rising after 0076** means the guards refused an unsafe merge — not a regression.

Related: **0080** (QC sampling per bind tier), **0081** (operator runbook).

### Identity flags (scan / dups / keep-set / unique-eml / unique-pst)

| Flag | Default | Direction |
|---|---|---|
| `--strong-content-hash <off\|body\|body-recip\|body-recip-attach>` | `off` | split (attach level is expensive full-stream I/O; **0086**) |
| `--strong-hash-attach-max-attaches` / `-max-bytes` / `-per-attach-max-bytes` | 50k / 1 GiB / 512 MiB | budgets for attach-content digests only |
| `--dedupe-scope <global\|per-source>` | `global` | split |
| `--tier1-verify <off\|content\|body>` | `off` | split |
| `--tier1-backfill` | off | **merge** (keep-set / unique-pst / unique-eml post-pass only; **rejected** on streaming `scan`/`dups` — DedupIndex cannot retro-merge already-emitted uniques) |
| `--identity-ignore-inline-attachments` | off | **merge** (MAPI inline: Content-ID / rendered-in-body / hidden) |
| `--allow-cross-mid-tier2` | off | **merge** (pre-0076) |
| `--allow-degenerate-tier2` | off | **merge** (pre-0076) |

Desk wizard: single **Strong content hash** checkbox → `body` level. Full enums stay CLI-only (**D-0076-gui**).

## Fidelity & residuals

- Writer fidelity: see `docs/pst-writer-fidelity-v1.md` (0068–0070, **0073** reason taxonomy).
- Operator residual: Outlook / `scanpst.exe` structural check on multi-GB artifacts (not CI DoD).
- Count invariant (full success): sum of messages across volumes == `keep_set.stats.unique`.
- **Attach soft-fail invariant:** `export.attachments_failed` == sum of fail-severity ledger accounting (histogram always complete; CSV may be truncated under the row cap).
- **Promote-on-attach-fail (Mode A, 0083/0084):** `--promote-on-attach-fail` (default **off**). Pre-write only: when a keep-set peer materializes with incomplete attaches (`stream_available == false`, explicit `is_cloud_link`, or fail-severity attach fidelity) and a ranked peer is complete, promote the complete peer **before** PST/EML write commits that family. Default remains **Mode C** (write best-effort, ledger fails). **Mode B** write-time mid-message promote is **permanently declined**. All peers incomplete → Mode C fallback on highest-ranked materializable (`decided_by=mode_c_fallback_all_peers_incomplete`); group is not dropped for soft attach incompleteness alone. Under default global scope this may select another custodian’s complete copy (**cross-custodian de-duplication** — Sedona term); see the eDiscovery runbook. `duplicate_sources` on Unique rows still lists the full group after promote. **0084 cloud/modern attaches (attachment-table only):** detected via NPMAP `AttachmentProviderType` and/or web-ref method signals → incomplete + ledger reason `ATTACH_CLOUD_LINK` with appended columns `cloud_provider`,`cloud_url`; unique-PST writes a **pointer/metadata attach row** (no invented binary; no network download). **0085 body-inline document-shaped cloud URLs** are ledged in `export_body_cloud_links.csv` and counted on `export_messages.body_cloud_link_count` — they do **not** set `is_attach_incomplete` / Mode A promote (**known gap:** Mode A will not prefer a peer with a physical attach over a peer that only has the same logical message as an HTML inline link). **0086 `body-recip-attach` is live** (closes **D-0076-attach-content**): full-stream attach digests bind into Tier-2.5 strong identity, so incomplete vs complete / different-bytes peers can split into different keep-set groups. Mode A only walks within one keep-set group and **cannot cross attach-content group splits**.
- **`export_attachments.csv` cloud columns (0084):** header appends `cloud_provider,cloud_url` (right of existing columns; empty when not CloudLink). Formula injection neutralization applies to free-text URL cells.
- **`export_body_cloud_links.csv` (0085/0088/0097):** multi-row hit-list of **document-shaped** SharePoint/OneDrive body URLs (action tokens `:w:`/`:x:`/`:p:`/`:b:`/`:u:`; Office/PDF extensions; `1drv.ms`; SafeLinks unwrap when nested target is document-shaped). **Hosts:** commercial plus US GCC High/DoD (`*.sharepoint.us`, `admin.onedrive.us`, `*.sharepoint-mil.us`, `*.dps.mil`); SafeLinks wrappers include `*.safelinks.protection.office365.us`. Do not expect body-cloud rows from `admin.onedrive.us` alone (admin/sync). GCC Moderate = commercial endpoints. 21Vianet (`*.sharepoint.cn`) excluded; `.microsoft` TLD residual **D-0088-usgovcloud-microsoft-tld**. SafeLinks→SP/OD unwrap is mainly historical (SP/OD no longer wrapped). Caps: 100k body window, 2048 URL length, 50 links/message (not raised in 0097). Query strings are **never** stripped on **kept full hits** (as-sent share context). Bare site roots / `:f:` folder shares are misses. Always written when the report pack exists (independent of `--attach-ledger`). **`truncated` is a row-type discriminator** (real vs marker), not per-URL truncation. Real rows: `reason=BODY_CLOUD_LINK`, `truncated=false`, full query, counted in `messages_with_body_cloud_links` / `body_cloud_links_total`. Honesty marker (**0097**, ≤1 per message, only when a document-shaped candidate was actually dropped): `truncated=true`, `link_index=4294967295` (`u32::MAX`), `reason` one or more of `BODY_CLOUD_LINK_WINDOW` / `BODY_CLOUD_LINK_MAX_LINKS_EXCEEDED` / `BODY_CLOUD_LINK_URL_TRUNCATED` (pipe-joined). Window-only zero-candidate bodies emit **0** CSV rows. Umbrella `BODY_CLOUD_LINK_TRUNCATED` is **gone** — update counsel greps. URL-over-length marker `cloud_url` is the first 2048-char prefix (tenant/path visible) and is **not** a live/kept URL (does not increment `body_cloud_links_total` or consume a slot of 50). Summary: `messages_with_body_cloud_links`, `body_cloud_links_total`, `body_cloud_link_truncated_messages` (dropped-candidate messages only), `body_scan_window_capped_messages` (bodies that exceeded 100k; `serde(default)`). **D-0085-sovereign-cloud-hosts** closed in 0088. **D-0097-body-cloud-truncate-honesty** closed in 0097. Window-edge bare dedupe uses the same `normalize_candidate` (trailing sentence punct + HTML unescape) as kept hits; classified over-length URLs join `seen` via `note_overlength` so in-window, edge, and `note_unseen_in` (including SafeLinks nested tail) share identity and do not emit an extra `BODY_CLOUD_LINK_WINDOW` (**0105**; closes **D-0097-window-edge-normalize**). Unique over-length URLs past the 50-hit cap still note `max_links`. No network fetch; no invented Attachment Table rows; body-only hits do not alone force exit 64.
- **unique-eml:** Mode A flag threads the shared materialize finalizer; full attach-ledger CSV parity closed in **0089** (`{out}/export_attachments.csv`, same header/flags as unique-pst).
- **GUI:** no attach-ledger / Mode A UI controls — residual **D-0073-gui** (CLI flags; wizard pass-through default false).
- CRC stderr noise is **0077**; ledger is not a dump of page CRCs.
- Deep attach preflight before export is **0074** (shared reason code strings with **0073**).
- **GUI deep-attach checkbox:** residual **D-0074-gui** (CLI `--deep-attach-preflight` works; wizard defaults off).

## Exit honesty & automation contract (0078)

Integrity thresholds, export partials, verification failures, and **`attachments_failed > 0`** still **flush the report pack** before any non-zero exit. With `--json`, the summary is printed on stdout even when `ok` is false.

**Severity order (not numeric order):** cancelled → hard fail → risk gate → partial fidelity → success.

| Code | Name | Meaning | Script action |
|---:|---|---|---|
| **0** | Success | Complete fidelity; nothing to review | proceed |
| **1** | Generic | Hard fail — artifact absent or untrustworthy | investigate; do not ship |
| **2** | Usage | Bad arguments | fix invocation |
| **3–5** | Busy / JobFailed / MatterIo | Frozen matter/service codes | as today |
| **64** | PartialFidelity | **Artifact exists and is message-complete;** attachment/body soft-failures recorded | review ledger; ship only with disclosure |
| **65** | ExportRiskBlocked | 0077 `export_risk` met `--fail-on-export-risk` | re-export from source; do not produce |
| **130** | Cancelled | Operator cancelled (SIGINT convention) | rerun; not an error |

**64 means the artifact exists and is message-complete.** Do not delete a usable PST solely because the shell said non-zero — inspect `fidelity` / `exit_reason` first.

### Flags

| Flag | Default | Effect |
|---|---|---|
| `--fail-on-partial-fidelity` | **on** (implicit) | Partial → exit **64** |
| `--allow-partial-fidelity` | off | Partial → exit **0**; JSON still `fidelity: partial` and `ok: false` with `error.code=partial_fidelity` |
| both fidelity flags | — | **exit 2** (usage) |
| `--fail-on-export-risk <ok\|re_export_recommended\|not_export_ready>` | **off** | When `export_risk` rank ≥ level → exit **65** |

### JSON fields (additive)

`fidelity`, `exit_code`, `exit_reason`, `artifact_state`, `summary_path` — all on every summary. `ok == (fidelity == complete)`. **`exit_code` must equal the process exit status.**

With `--also-eml`, `fidelity` / `ok` / `exit_code` describe the **combined** unique-pst + also-eml job (worse classified fidelity; 0078 exit precedence). `artifact_state` remains the PST `--out` disposition only (also-eml cancel does not quarantine PST volumes). The also-eml pack keeps its own `{also_eml}/summary.json` (including pack `fidelity`).

`--allow-partial-fidelity` unique-pst summaries with `fidelity=partial` carry `ok=false` and `error.code=partial_fidelity` (`retryable` stays `false`). Automation must read `fidelity` / `ok`, not treat a missing `error` key or exit 0 as complete.

`artifact_state`: `complete` | `partial_retained` | `partial_quarantined` | `invalid_in_place` | `absent`.
- **`partial_retained`**: message-complete soft-fail deliverable (exit 64) — ship only with disclosure.
- **`invalid_in_place`**: bytes still at `--out` but **must not ship** — cancel quarantine failed, **or** hard-fail after write (incomplete / untrustworthy). Purge or quarantine manually before retry.
- Spec has no `failed_retained`; hard-fail with retained bytes uses `invalid_in_place`.

**`retryable` (0082):** boolean on summary JSON only — **not** a new exit integer. `true` only for clearly transient classes (operator cancel, matter/transient IO). Permanent failures (usage, risk gate, partial fidelity, verify/count/report hard fails, schema/passphrase/audit) stay `false`. Still **no blanket retry of exit 5** — classify first (see [runbook](unique-pst-ediscovery-runbook.md) §6).

**0082 summary counters (telemetry):**

| Field | Meaning |
|---|---|
| `bcc_suppressed_message_count` | Winners where source BCC was omitted from the written PST by policy |
| `sent_message_with_no_recipients_count` | Empty recipient TC on a non-draft (`MSGFLAG_UNSENT` clear) — **telemetry only**; does not invent a new `export_risk` value or hard-fail the export |

### Cancel quarantine

On cancel after bytes written, volumes are renamed to
`{filename}.cancelled-{unix_secs}-{millis}.partial`
(e.g. `unique.pst` → `unique.pst.cancelled-1720000000-042.partial`) so `--out` is free for a plain retry (no `--overwrite`). Never deleted. If that name already exists, a collision suffix is used (`_2`, `_3`, …); existing quarantine files are never overwritten.

### PowerShell dispatch example

```powershell
.\target\release\pst-dedup.exe unique-pst $src --out $out --report-dir $report --json
$code = $LASTEXITCODE
switch ($code) {
    0   { "complete — proceed" }
    64  { "partial fidelity — review attach ledger; artifact retained" }
    65  { "export risk gate — re-export from source" }
    130 { "cancelled — retry when ready" }
    1   { "hard fail — do not ship" }
    2   { "usage — fix args" }
    default { "other: $code" }
}
```

Cross-links: **0080** QC scripts branch on these codes; **0081** runbook (do **not** blanket-retry exit 5 — `MatterIo` includes `AuditChainBroken`).

## GUI wizard (`pst-dedup-gui`, track 0072)

Operators who prefer a desktop UI can run the **same** orchestration without CLI:

1. **File select** → **Unique PST Export…**, or **Results** → **Export Unique PST…** (primary unique path after a scan).
2. **Select** sources (main-thread multi-file picker).
3. **Options** — Save File for `.pst` out, report dir, policy / family / mode / max-volume / overwrite.
4. **Run** — stage, counters, physical size, **Cancel**, expandable **Log / Details** (stderr parity).
5. **Done** — ok / partial / cancelled; open report or output folder.

| Behavior | Notes |
|---|---|
| Pipeline | In-process `run_unique_pst_with_options` (not a second path; not `pst-dedup.exe` spawn) |
| Cancel | Cooperative `AtomicBool`; incomplete volume temp deleted; completed multi-volumes retained; report pack flushed with `ok=false` when possible |
| Progress | Worker calls `ctx.request_repaint()` on progress ticks so the bar updates without mouse motion |
| Log | Non-fatal warnings and stage lines appear in Log / Details |
| Legacy EML | Still available on Results as **Export Unique EML (legacy scan path)**; unique-PST is preferred (**D-0067-gui-keepset** soft-closed) |

See also CLI flags above — wizard maps to the same `UniquePstCliArgs` fields.

## Output QC (track 0080)

Source-differential QC replaces self-referential verify as the durable proof that the
deliverable matches its sources. External tools (scanpst, libpff/libpst) are optional
corroboration only.

| Level | What it does |
|---|---|
| `off` | Legacy open+count+sample only |
| `structure` | Folder **tree** + message counts vs expected keep-set layout |
| `sample` *(default)* | Structure + risk-weighted source↔output content/attach compare (cap `--qc-sample-max`) |
| `full` | Structure + every written message compared |

Hard findings (`defect`, `unexplained_loss`) set `verify_ok = false` → existing
`VERIFY_FAILED` / exit **1**. `known_gap` (e.g. BCC dropped by design unless
`--include-bcc-recipients`) is counted and never fails. `recipient_table` is
**Preserved** (0082) — a source↔output structure mismatch is a defect. No new exit integers.

Standalone re-check:

```powershell
.\target\release\pst-dedup.exe qc-pst C:\export\unique.pst --report-dir C:\export\unique_report --json
```

When sources are gone, `qc-pst` is structural-only unless `content_digests.json` was
persisted at export time (`content_digest_backed: true`). The tool never self-attests a
human Outlook open — operators may drop `qc_attestation_v1.json` into the report dir.

### Client-retirement honesty (re-verified 2026-07-29)

| Fact | Detail |
|---|---|
| Classic Outlook | Opt-out-default since **April 2026**; retires **Q1–Q2 2028**; **EOL Q2 2029**. `scanpst.exe` is classic-only. |
| New Outlook | Default client. **Can open/add** `.pst` (Settings → Files → Outlook Data Files → Add file) per Microsoft Support (access date **2026-07-29**); classic Outlook must also be installed, same bitness. **No COM/VSTO/VBA** object model planned. Stale “import-only / not mount” claims are **incorrect** for current open/add email browse. |
| Microsoft PST roadmap | Microsoft has stated limited future investment in `.PST` once bulk import ships. Human client open is still available today but is **not** a durable automation surface. |
| Consequence for proof | Tier C (`scanpst`) has a shelf life; do not treat “open it in Outlook” as the primary proof. **PST remains a correct deliverable** (Purview exports it; eDiscovery consumes it) — durable QC is **source-differential reader QC**, not a Microsoft client. |
| COM automation | Declined (D-0080-com-declined): no future on new Outlook; mutates operator env; scanpst is strictly better for format validation. |

Cross-link: **0081** runbook depends on both the exit contract (0078) and this QC pack.
