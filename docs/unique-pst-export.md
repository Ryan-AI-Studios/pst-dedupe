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
| `--json` | Summary JSON on **stdout**; human progress on **stderr** |

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
  summary.json           # unique_export_report_v1
  decisions.csv          # keep-set decision stream
  keepset.json           # winners + stats (no bodies)
  volumes.csv            # one row per completed volume (+ sha256/md5)
  export_messages.csv    # MANDATORY winner → volume cross-reference
  integrity.csv          # optional / if requested
```

### `export_messages.csv` (mandatory)

Fixed columns (order locked):

```text
source_path,folder_path,nid,message_id_norm,edrm_mih,content_hash_hex,volume_path,volume_index,export_message_index
```

One row per **successfully written** unique winner. **No body text** columns.

### Default hash trust vs `--verify-hash`

- **Default:** report digests come from the writer (`WritePstReport`); Phase 5 does **not** re-read multi-GB files solely to rehash.
- **Structural proof:** open with `pst-reader`, message count == `messages_written`, sample ≥ min(5, N) Message-IDs.
- **`--verify-hash`:** independent full-file SHA-256; sets `verification.hash_match` (use on small fixtures / CI).

## Fidelity & residuals

- Writer fidelity: see `docs/pst-writer-fidelity-v1.md` (0068–0070).
- Operator residual: Outlook / `scanpst.exe` structural check on multi-GB artifacts (not CI DoD).
- Count invariant (full success): sum of messages across volumes == `keep_set.stats.unique`.

## Exit honesty

Integrity thresholds, export partials, and verification failures still **flush the report pack** before non-zero exit. With `--json`, the summary is printed on stdout even when `ok` is false.

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
