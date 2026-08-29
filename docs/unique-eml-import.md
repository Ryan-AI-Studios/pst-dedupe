# Unique EML pack — operator import guide

Track **0067** ships `pst-dedup unique-eml`: a **keep-set-driven** directory of unique
messages as RFC 5322 `.eml` files for import into Outlook, Thunderbird, or other
mail clients. This is the Series K **interim** path while production PST write
(0068–0070) lands.

## 1. Produce the pack

```powershell
.\target\release\pst-dedup.exe unique-eml a.pst b.pst `
  --out C:\Cases\Matter1\unique_eml_pack `
  --policy first_seen `
  --decision-csv C:\Cases\Matter1\decisions.csv `
  --keep-set-json C:\Cases\Matter1\keepset.json `
  --json
```

`unique-pst --also-eml <dir>` writes the same unique-EML pack layout from the unique-pst keep-set (no second scan).

| Flag | Notes |
|---|---|
| `--out` | Pack root (required). Refuses non-empty dirs unless `--overwrite`. |
| `--files-per-volume` | Default **10000** EML files per volume folder (clamped 1000–50000). |
| `--volume-prefix` | Default `VOL` → `VOL001`, `VOL002`, … |
| `--family-policy parents_only` | Parent messages only — **no** attachment/embedded MIME parts. |
| `--attach-ledger full\|summary-only\|off` | **0089** — attachment failure ledger (default **`full`**). CSV at `{out}/export_attachments.csv`. |
| `--attach-ledger-max-rows <N>` | Cap on CSV rows (default **500000**); histogram never truncated. |
| `--ledger-path-mode full\|basename` | How `source_path` is written in the attach ledger (default **`full`**). |
| `--max-embedded-depth <1-8>` | **0106** — nested `ATTACH_EMBEDDED_MSG` extract/write depth (default **3**). Values outside **1–8** are a usage error. Deeper nests ledger `ATTACH_DEPTH_LIMIT`. |
| Integrity flags | Same as `scan` / `keep-set` (`--mode`, thresholds, `--allow-failed-files`). |

**Locks:**

- **No re-dedupe** — winners are post-promotion keep-set uniques only.
- **Source PSTs are read-only.**
- **Date header is always UTC `+0000`** (host timezone is ignored).
- Success invariant: **`eml_written == unique`**.

## 2. Pack layout

```text
{out}/
  manifest.json              # eml_pack_v1 (authoritative audit)
  summary.json               # fidelity / exit / attach ledger summary
  export_attachments.csv     # 0089 attach failure ledger (mode=full)
  VOL001/
    000001_<id>_<subject>.eml
    …
  VOL002/
    …
```

- Files are **volume-batched** so Explorer / AV / backup stay usable at 100k+ messages.
- Filenames are deterministic (`counter` + EDRM MIH or content-hash fragment + safe subject).
- Absolute paths stay within a **≤250** character budget (subject truncated first).

## 3. Review before import

1. Open `{out}/manifest.json` — check `stats.eml_written`, `degraded_messages`,
   `attach_parts_failed`, `embedded_messages_written`.
2. Open `{out}/summary.json` — check `fidelity` / `exit_code` / `attachment_ledger*`.
3. When attaches failed, review `{out}/export_attachments.csv` (same columns as unique-pst).
4. Review decision CSV for `dup_of` / `materialize_failed` rows.
5. Spot-check a few `.eml` files (Date ends with `+0000`; attachments present when expected).

## 4. Import into Outlook (manual)

1. Create or open the target mailbox / PST in Outlook.
2. For **each** `VOL###` folder under the pack root:
   - File → Open & Export → Import/Export → **Import from another program or file**
   - Or drag-drop `.eml` files into a folder (Outlook version dependent).
3. Do **not** expect Explorer to browse 300k files in one directory — that is why
   we volume-batch.
4. Optional: create a **new empty PST** in Outlook and move imported mail there
   for an interim “clean” store without our PST writer.

## 5. Import into Thunderbird (manual)

1. Install the **ImportExportTools NG** add-on (or equivalent).
2. Import each `VOL###` directory as EML files into a local folder.
3. Optionally copy into an IMAP account or archive.

## 6. Honesty notes

| Topic | Reality |
|---|---|
| Round-trip | **Not** bit-identical to original MIME — reconstructed from MAPI properties. |
| Date | Always **UTC +0000** for reproducibility across operator machines. |
| Embedded messages | When nested extract succeeds, the `message/rfc822` body is reconstructed RFC 5322 (not a MAPI dump). Extract/depth failures skip the part and ledger `ATTACH_EMBEDDED_UNPARSED` / `ATTACH_DEPTH_LIMIT`. By-value attached `.eml` (method ≠ 5) is still dumped as rfc822. Deep matter/Relativity child documents remain residual (`D-0067`). |
| Cloud/modern attaches | Hyperlink-only / cloud attaches are not downloaded (residual). |
| Degraded winners | Still exported with `X-Pst-Dedupe-Degraded` + manifest flags. |
| Partial pack | Non-zero integrity exit still flushes written EML + manifest stats. |

**Attach ledger CSV (`export_attachments.csv`):** Unmapped EML soft-fail causes map to
reason_code `ATTACH_UNKNOWN` (the row is never dropped). CSV `reason_code` values use the
unique-pst **0073** taxonomy; pack-manifest aggregate `ATTACH_PART_FAILED` is **not** used
as a CSV reason_code.

## 7. Related

- CLI keep-set: `pst-dedup keep-set` (plan uniques without writing EML)
- Full guide context: `conductor/How-to-use.md` §2.5
- Production PST write: tracks **0068–0070** (later)
