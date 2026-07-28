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

Fixed columns (prefix locked; **0073** appends one column at the end):

```text
source_path,folder_path,nid,message_id_norm,edrm_mih,content_hash_hex,volume_path,volume_index,export_message_index,attachments_failed_count
```

One row per **successfully written** unique winner. **No body text** columns.

`attachments_failed_count` is the number of **fail-severity** attach outcomes for that message on the write path (left-join alternative: count fail rows in `export_attachments.csv` for the same `source_path` + `msg_nid`).

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
