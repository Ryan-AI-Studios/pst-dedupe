# Unique PST export (`pst-dedup unique-pst`)

Headless operator path (Series K / track **0071**): multi-input PSTs → keep-set winners → streaming unique PST volume(s) → defensible report pack + verification.

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
2. **`keep_set_v1` resolve** + **`finalize_with_materialize`** (promote only)
3. **Streaming write** via `write_unicode_pst_streaming` (attachments streamed; never re-dedupe)
4. **Report pack** under `--report-dir`
5. **Verify** each completed volume (open + count + sample MID; optional `--verify-hash`)

Source PSTs are **read-only**. The writer never mutates inputs.

## Flags

| Flag | Notes |
|---|---|
| `--out <path>` | **Required** — primary PST (volume 1) |
| `--report-dir <dir>` | Default: sibling of `--out` stem + `_report` (e.g. `unique.pst` → `unique_report`) |
| `--input` / positionals | One or more source PSTs |
| Keep-set / integrity | Same family as `unique-eml` (`--policy`, `--family-policy`, `--mode`, thresholds, …) |
| `--policy earliest_date` | Prefer earliest submit time (delivery fallback; missing last). See [Winner policies](#winner-policies). |
| `--prefer-bcc-copy` | Prefer copy with non-empty PidTagDisplayBcc (sender-copy completeness) |
| `--prefer-folder-class` | Built-in folder ladder (Sent Items > live > … > Recoverable Items) |
| `--folder-rank <PATTERN>` | Custom folder demotion ladder (repeatable, worst-last; segment globs; replaces built-in) |
| `--source-rank <SUBSTRING>` | Ordered source preference (repeatable, best-first; unmatched worst) |
| `--rank-folder-class-first` | Swap source_rank ↔ folder_class rungs |
| `--fidelity-rank binary\|graded` | Binary (default, pre-0075) or multi-tier graded fidelity |
| `--folder-layout` | `preserve` (default) or `flat` |
| `--max-volume-bytes` | Soft physical-size ceiling; **off** = single volume |
| `--overwrite` | Required to replace existing `--out` / non-empty report-dir |
| `--verify-hash` | Full-file rehash vs report digests (default **off** for multi-GB comfort) |
| `--also-eml <dir>` | Soft residual (accepted; co-export may be ignored — see deferred) |
| `--attach-ledger full\|summary-only\|off` | **0073** — attachment failure ledger (default **`full`**) |
| `--attach-ledger-max-rows <N>` | Cap on `export_attachments.csv` rows (default **500000**); histogram never truncated |
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
  summary.json              # unique_export_report_v1 (+ 0073 attach histogram fields)
  decisions.csv             # keep-set decision stream
  keepset.json              # winners + stats (no bodies)
  volumes.csv               # one row per completed volume (+ sha256/md5)
  export_messages.csv       # MANDATORY winner → volume cross-reference
  export_attachments.csv    # 0073 attach failure ledger (mode=full)
  integrity.csv             # optional / if requested
```

### Sensitivity (handoff)

The entire **`report-dir` is operator-sensitive**: absolute paths, folder names, subjects, and attachment filenames can leak PII or privilege context. Do **not** post report packs to untrusted third parties without redaction. Prefer sharing the summary histogram + reason codes first. Primary join key that avoids path strings: **`source_id`** (0-based index into `summary.inputs`). Optional basename redaction mode is residual (**D-0073-basename**).

### CSV open safety

Free-text cells in `export_attachments.csv` and `export_messages.csv` neutralize spreadsheet formula injection: values whose leading non-space character is `=`, `+`, `-`, or `@` are prefixed with `'` before CSV quoting. Still open CSVs as text when possible; treat the pack as sensitive.

### `export_messages.csv` (mandatory)

Fixed columns (prefix locked; **0073**/**0075** append only):

```text
source_path,folder_path,nid,message_id_norm,edrm_mih,content_hash_hex,volume_path,volume_index,export_message_index,attachments_failed_count,duplicate_source_count,duplicate_sources
```

One row per **successfully written** unique winner. **No body text** columns.

`attachments_failed_count` is the number of **fail-severity** attach outcomes for that message on the write path (left-join alternative: count fail rows in `export_attachments.csv` for the same `source_path` + `msg_nid`).

`duplicate_source_count` / `duplicate_sources` (**0075**): “All Custodians” aggregate — distinct **source PST basenames** (not absolute paths) that held a suppressed copy of this winner, `|`-delimited, capped at 8 (same values as decision CSV unique rows and `keep_set_v1` JSON).

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
| `distinct_bad_bids` large / `exact=false` | widespread block corruption | re-export before trusting the unique set |
| `crc_suspect_messages` > 0 | documents **kept with possibly-wrong bytes**; held out of Tier 2 by default | higher unique count expected; flagged, not lost |
| `block_crc_read_rate` ≥ 0.15 | the medium is failing, not the mailbox | `export_risk = not_export_ready` — re-image or re-export |
| `attach_fail_rate` over threshold | attachment payloads unreadable | re-export; the 0073 ledger names which |
| `export_risk = not_export_ready` | failed volume, catastrophic rate, or scan already said so | do not hand off |

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

**`decided_by` vocabulary:** `sole_member`, `fidelity`, `bcc_completeness`, `source_rank`, `folder_class`, `policy_first_seen`, `policy_keep_largest`, `policy_prefer_path`, `policy_earliest_date`, `path_order`, `nid`, `promoted_after_materialize_fail`.

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
| `winner_promoted` | Always `false` in P0 (Mode A promote residual **D-0073-promote**) |

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
| **2.5** (v2) | v1 preimage **plus** layered extras | Off — `--strong-content-hash body\|body-recip` (`body-recip-attach` deferred **D-0076-attach-content**) |

**Named divergences from Relativity’s four-component hash:**

| Component | Relativity | This tool (v1 default) |
|---|---|---|
| Body | Full `PR_BODY` with CR/LF/space/tab stripped | First **4096 characters** of whitespace-normalized body (spaces kept) |
| Header | Subject + sender name/email + ClientSubmitTime | Subject + sender email + submit FILETIME (no separate display name) |
| Recipients | All recipients incl. BCC (address-oriented) | **Absent at v1**; opt-in at `body-recip` via display strings |
| Attachments | Per-attachment **content** SHA-256 | Name + size metadata only (content digests deferred or opt-in attach level) |

### Split-only guards (default on)

1. **Unreadable / degenerate body** — items with `body_incomplete` / `body_unavailable`, or no body and fewer than two weak fields (subject / time / sender / ≥1 attach), do **not** bind on Tier 2. Escape: `--allow-degenerate-tier2`.
2. **Cross-MID** — two items with **different** non-empty Message-IDs never merge on content hash. Escape: `--allow-cross-mid-tier2`.

**Bulk-mail warning:** blocking cross-MID merges inflates unique counts most for newsletters, HR templates, and automated mailers (each dispatch has its own Message-ID). Read `cross_mid_blocked_max_group` in the run summary first. Use `--allow-cross-mid-tier2` only when aggressive bulk culling is intentional and recipient-level evidence is not at issue.

**Recipient warning (`body-recip`):** `display_to` / `cc` / `bcc` are *display names*, not SMTP addresses, and vary between copies (`"Smith, John"` vs `"John Smith"` vs `/O=EXCHANGELABS/…`). Check `tier2_5_splits_recipients_only` and `x500_recipient_items` before trusting results. Long-term fix is recipient-table reads (**D-0076-recipient-table**).

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
| `--strong-content-hash <off\|body\|body-recip>` | `off` | split (`body-recip-attach` rejected until **D-0076-attach-content**) |
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
- **Promote-on-attach-fail (Mode A):** not shipped in 0073 — default is **Mode C ledger-only** (write best-effort message, ledger fails, `ok=false`). Residual **D-0073-promote**. Write-time mid-message promote is out.
- **unique-eml:** no attach ledger CSV in this track — residual **D-0073-eml** (operators use unique-pst ledger or re-run unique-eml with pack logs).
- **GUI:** no attach-ledger UI controls — residual **D-0073-gui** (CLI flags work via shared args).
- CRC stderr noise is **0077**; ledger is not a dump of page CRCs.
- Deep attach preflight before export is **0074** (shared reason code strings with **0073**).
- **GUI deep-attach checkbox:** residual **D-0074-gui** (CLI `--deep-attach-preflight` works; wizard defaults off).

## Exit honesty

Integrity thresholds, export partials, verification failures, and **`attachments_failed > 0`** still **flush the report pack** before non-zero exit (`ok=false`). With `--json`, the summary is printed on stdout even when `ok` is false.

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
