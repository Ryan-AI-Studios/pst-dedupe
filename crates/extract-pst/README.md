# extract-pst

Blocking library that opens **Unicode PST** evidence (filesystem and/or matter
CAS), walks folders/messages/attachments via **`pst-reader`**, and writes
**Normalized Items** into `matter-core`.

## ⚠️ BLOCKING THREAD WARNING

`extract_pst_item`, `resume_extract`, and `extract_pst_path` are **CPU- and
IO-bound** and block for the duration of the walk. Callers **must** run them on
a dedicated blocking worker (`std::thread`, rayon, or
`tokio::task::spawn_blocking` in track 0019+). Calling them on the GUI thread or
a Tokio async worker will freeze the Desk.

This crate does not enforce that contract.

## Public API

| Function | Purpose |
|---|---|
| `extract_pst_item(matter, source_id, pst_item_id, limits, cancel)` | Extract using a 0016 inventory row |
| `extract_pst_path(matter, source_id, path, limits, cancel)` | Register FS PST + extract |
| `resume_extract(matter, source_id, job_id, limits, cancel)` | Resume mid-folder checkpoint |
| `list_discovered_psts(matter, source_id)` | Inventory rows whose path ends in `.pst` |

Job kind: `extract_pst`. Stage: `pst_extract`. Default `batch_size`: **500**
(mid-folder checkpoints).

## Path convention

```text
{pst_inventory_path}!/{folder_path}/{message_nid_hex}
{pst_inventory_path}!/{folder_path}/{message_nid_hex}/attach/{index}_{safe_filename}
```

Resume / re-extract skip key: `(source_id, path)` via `item_by_source_path`.
If **any** item already exists for a message path (`…!/…`), extract **skips**
that path and never double-inserts (covers `extracted`, `partial`, and prior
error rows). Retry-with-update is deferred until unique path upsert exists.

## Native identity (`native_sha256`)

| Object | Native bytes in CAS |
|---|---|
| **Email parent** | Deterministic **`pst-native-message-v1`** blob (magic `PNM1` + version + fixed field order) |
| **Attachment** | Raw attachment payload stream from the PST |

**Not used for `native_sha256`:** synthetic EML (deferred to **0040** Production
Export). EML formatters are lossy; fixing a header encoding would change digests
and break chain-of-custody.

`extra_json` records `"native_format": "pst-native-message-v1"`.

## Streaming attachments

Attachment binary is opened as `std::io::Read` (`pst_reader::AttachmentDataReader`)
and written via `Matter::put_reader` / `Cas::put_reader` with a 64 KiB buffer.
Production path does **not** require a full multi-GB `Vec<u8>`.

## Matter-local temp (never `%TEMP%`)

When the inventory PST exists only in CAS, bytes are materialised under:

```text
<matter-root>/workspace/temp/{jobid}_{digest12}_{seq}_{pid}.pst
```

- Unique names (job id + digest prefix + random/seq)
- RAII delete on drop (success and error paths)
- `Matter::open` / `Matter::create` wipe leftover `workspace/temp/` contents
- **Never** `std::env::temp_dir()` for evidence

Open order: filesystem path if present → else CAS → `workspace/temp/`.

## Recipients / BCC

DisplayTo / DisplayCc / DisplayBcc semicolon lists are parsed best-effort.
**BCC is never invented** — missing property → empty `[]` in JSON and empty
`Vec` in `EmailLogicalInput` (still framed in logical_hash v1).

## Logical hash

Always calls `matter_core::compute_email_logical_hash` after attachment digests
are known. Does **not** reimplement preimage framing.

## Checkpoints / resume

After every `batch_size` messages **including mid-folder**:

```json
{
  "last_folder_path": "…",
  "last_message_nid": "hex",
  "folder_message_index": 40500,
  "completed_count": 41200,
  "batch_size": 500
}
```

Cancel → job `Paused` + durable checkpoint; `resume_extract` continues.

`max_messages` is a **safety cap for this run**, not a claim of full extract.
If the cap is hit mid-PST (more messages remain), the job is **`Paused`** with
`completed: false` and a checkpoint — use `resume_extract` (or raise the cap)
to continue. Only a full folder walk sets `Succeeded` / `completed: true`.

When the cap pauses the run, audit emits **`extract.paused`** (reason
`max_messages`) — not `extract.complete`. Cancel leaves the job `Paused` with a
checkpoint and does **not** emit `extract.complete` either.

Resume fails closed (`resume_pst_mismatch`) if the checkpoint `pst_path` or
`pst_native_sha256` no longer matches the inventory item.

## Errors (structured codes)

`pst_open_failed`, `pst_ansi_rejected`, `message_props_failed`,
`attach_list_failed`, `attach_data_missing`, `attach_too_large`,
`cas_put_failed`, `resume_pst_mismatch`, `cancelled`.

Per-message continue; `item_errors` for partials (including attachment-table
enumeration failures → parent `partial` + `attach_list_failed`). Audit:
`extract.start` / `extract.complete` / `extract.paused` / `extract.fail`
(not per-message).

## Out of scope

- EML as native identity (0040)
- Mutating source PST / Purview trees
- Job runner / progress channels (0019)
- Matter-wide dedupe (0021)
- Recipient table (MAPI) vs Display* only (P0 uses Display*)
- Parallel folder extract
