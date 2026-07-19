# extract-pst

Blocking library that opens **Unicode PST** evidence (filesystem and/or matter
CAS), walks folders/messages/attachments via **`pst-reader`**, and writes
**Normalized Items** into `matter-core`.

## Calendar messages (0035)

When `PidTagMessageClass` (`0x001A`) is a calendar class, the parent item is
written with `file_category=calendar` (not `email`):

| Class | Path |
|---|---|
| `IPM.Appointment` | calendar |
| `IPM.Schedule.Meeting.Request` | calendar |
| `IPM.Schedule.Meeting.Resp.*` | calendar |
| `IPM.Schedule.Meeting.Canceled` | calendar |
| `IPM.Note` / other | existing email path (**unchanged**) |

Standard tags only (P0): `PidTagStartDate` `0x0060`, `PidTagEndDate` `0x0061`,
best-effort `PidTagLocation` `0x3A0D`. Full **PidLid*** named-prop map
(AppointmentStartWhole, BusyStatus, RecurrencePattern, …) is residual.

- `cal_extract_method` = `pst_oxocal_v1`
- Review body is synthesized (Subject/When/Where/Organizer/Attendees/Class/description)
- Pure calendar without Message-ID uses `compute_non_email_logical_hash`; meeting
  requests that carry Message-ID keep the email MID logical_hash path
- `sent_at` falls back to `cal_start_at` when submit time is missing

## Threading headers (0022)

Parent email rows store reply-chain fields when present on the message:

| Column | Source |
|---|---|
| `in_reply_to` | PidTagInReplyToId `0x1042` (normalized MID) |
| `references_json` | PidTagInternetReferences `0x1039` (unfold + `<…>` parse) |
| `conversation_topic` | PidTagConversationTopic `0x0070` |
| `conversation_index_hex` | PidTagConversationIndex `0x0071` (bytes or Base64 → lowercase hex) |

Missing props stay **NULL** (never fabricated). Matters extracted **before**
this track lack these columns until **re-extract**. Re-extract of an existing
`(source_id, path)` **refreshes** these four header columns (headers-only
update — no double-insert, no body re-CAS).

## ⚠️ BLOCKING THREAD WARNING

`extract_pst_item`, `extract_pst_item_on_job`, `extract_pst_path`,
`extract_pst_path_on_job`, and `resume_extract` are **CPU- and IO-bound** and
block for the duration of the walk. Callers **must** run them on a dedicated
blocking worker — preferably the **0019** `process-runner` matter worker.
Calling them on the GUI thread or a Tokio async worker will freeze the Desk.

This crate does not enforce that contract.

## Job-id authority (Option C)

Orchestrated runs: **`process-runner` creates the job**, then calls
`extract_pst_item_on_job` / `extract_pst_path_on_job` (no internal
`create_job`). Public wrappers create a job then call the on-job path.
`resume_extract` already takes an existing `job_id`.

## Public API

| Function | Purpose |
|---|---|
| `extract_pst_item(...)` | Create job + extract inventory PST (wrapper) |
| `extract_pst_item_on_job(..., job_id, ...)` | Extract on **pre-created** job_id |
| `extract_pst_path(...)` | Create job + register FS PST + extract (wrapper) |
| `extract_pst_path_on_job(..., job_id, ...)` | Path extract on **pre-created** job_id |
| `resume_extract(matter, source_id, job_id, limits, cancel)` | Resume mid-folder checkpoint |
| `list_discovered_psts(matter, source_id)` | Inventory rows whose path ends in `.pst` |

Job kind: `extract_pst`. Stage: `pst_extract`. Default `batch_size`: **500**
(mid-folder checkpoints).

## Path convention

```text
{pst_inventory_path}!/{folder_path}/{message_nid_hex}
{pst_inventory_path}!/{folder_path}/{message_nid_hex}/attach/{index}_{safe_filename}
```

Resume / re-extract key: `(source_id, path)` via `item_by_source_path`.
If **any** item already exists for a message path (`…!/…`), extract **does not
double-insert** (covers `extracted`, `partial`, and prior error rows) but
**does re-read** the message to refresh the four threading header columns
(`in_reply_to`, `references_json`, `conversation_topic`,
`conversation_index_hex`). Full field retry-with-update is deferred until
unique path upsert exists.

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
