# 0018 — PST extractor adapter (wrap pst-reader)

- **Track ID:** 0018-PstExtractorAdapter
- **Execution repo:** `C:\dev\dedupe`
- **Governance:** this directory in `C:\dev\dedupe\conductor\`
- **Plan-of-record reference:** `C:\dev\Dedupe-plan.md` → §2.2–2.3, §4.4 (`extractors`), §4.6 (blocking pool + PST checkpoint grain), Series A / **018**, §5.2
- **Cross-repo contract:** n/a
- **Status:** Ready — not started
- **Depends on:** **0016-PurviewIngest** (Completed), **0017-NormalizedItem** (Completed — schema v2 + family + `logical_hash` v1)

---

## 1. Objective

Build a **blocking** library that opens **Unicode PST** evidence (from filesystem and/or matter CAS), walks folders/messages/attachments via **`pst-reader`**, and writes **Normalized Items** into `matter-core`:

- Parent email items + attachment children in **`email_attachments`** families
- **`native_sha256`** = CAS digest of **physical** bytes written for that object
- **`logical_hash`** / **`logical_hash_version`** via existing `matter_core::compute_email_logical_hash` (BCC-aware, length-prefixed v1 — **already shipped in 0017**)
- Resumable **extract** checkpoints (folder and/or every N messages + last NID)
- Honest partial success (`item_errors`, per-message continue)

This is the bridge from “PST discovered by 0016” → “reviewable items for 0021+”.

---

## 2. Context (read before starting)

### 2.1 Plan-of-record

- `C:\dev\Dedupe-plan.md` §2.2–2.3 (item fields + native vs logical), §4.6 (PST extract checkpoint: folder **or** every N messages, default 500 + last NID/path), Series A track **018**.
- Guardrails: `../TRACK-GUARDRAILS.md`.
- Sequencing: `../sequencing.md` — after **0016 + 0017**; unblocks **0021** (with 0019).

### 2.2 What 0016 delivered (inputs)

| Fact | Use in 0018 |
|---|---|
| `ingest-purview` expands packages; **`.pst` leaves** get inventory rows | Find extract targets: `path` ends with `.pst`, `status` often `discovered`, `native_sha256` = **whole-file** PST bytes in CAS |
| Nested path style | e.g. `files.zip!/mail.pst` — message paths must nest under this |
| Whole PST may only exist in **CAS** (or still on disk at package root) | Must open PST from **filesystem path** when available **or** materialize from CAS |
| Does **not** parse messages | 0018 owns message/attachment extract |
| Blocking-thread contract | Same pattern for extract APIs |

### 2.3 What 0017 delivered (outputs to fill — **do not reimplement**)

| Surface | Use in 0018 |
|---|---|
| Schema **v2** | All P0 item columns |
| `insert_item` / `update_item` / `ItemInput` / `ItemUpdate` | Persist extracted fields |
| Family: `insert_family`, `set_item_family_role`, `list_attachments` | Parent email ↔ attachments; family cohesion enforced |
| `compute_email_logical_hash` + `EmailLogicalInput` | Identity after fields + child digests known |
| Logical hash v1 | Length-prefixed preimage; **BCC always framed**; RE/FW kept in subject |
| `item_status::EXTRACTED` / `PARTIAL` / `ERROR` | Lifecycle |
| `item_role::{PARENT, ATTACHMENT, STANDALONE}` | Roles |
| `text_sha256` / `html_sha256` | CAS digests of body text (0018 puts bytes) |
| Indexes | `logical_hash`, `message_id` ready for 0021 |
| Deferred (not 0018) | Unique `(source_id, path)`; formal FK on `parent_item_id`; relational participants |

**0017 is complete.** This track **calls** `matter-core` hash helpers; it does **not** patch preimage framing or re-open the BCC design debate unless a real regression is found.

### 2.4 What `pst-reader` has **today** (research — gaps to close)

| Capability | Status | 0018 implication |
|---|---|---|
| `PstFile::open(path)` | Yes | Need **bytes/temp open** if only CAS |
| `folders()` → path, `message_nids` | Yes | Walk + checkpoint by folder/NID |
| `read_message_properties` | **Dedup-oriented** | Message-ID, subject, submit time, sender, **body truncated to 4KB**, `display_to` only, size, has_attachments |
| To / Cc / **Bcc** as separate lists | **Missing** | Extend reader and/or parse DisplayTo/Cc/Bcc PIDs — **BCC required for logical_hash inputs** |
| Full body / HTML | Body read then truncated; no HTML PID wired | Prefer **full** `PidTagBody` for logical body; optional HTML → `html_sha256` |
| Received time | **Missing** | Add `PidTagMessageDeliveryTime` (0x0E06) when present |
| Attachment **metadata** (name + size) | Yes | Not enough for `LogicalAttachment.native_sha256` |
| Attachment **binary** | **Missing** | Must read attach data into CAS (method-dependent) |
| ANSI PST | Rejected | Surface structured error |

**PID constants present today** (extend as needed):  
`INTERNET_MESSAGE_ID`, `SUBJECT`, `CLIENT_SUBMIT_TIME`, `SENDER_*`, `BODY`, `DISPLAY_TO`, `MESSAGE_SIZE`, `HAS_ATTACHMENTS`, attach filename/size.

**Likely additions in this track (on `PstFile` preferred):**  
`DISPLAY_CC` (0x0E03), `DISPLAY_BCC` (0x0E02), `MESSAGE_DELIVERY_TIME` (0x0E06), `BODY_HTML` (0x1013), attach `ATTACH_DATA_BINARY` (0x3701), `ATTACH_METHOD` (0x3705), `ATTACH_MIME_TAG` (0x370E), etc.

### 2.5 Existing crates (boundaries)

| Crate | Role |
|---|---|
| **`pst-reader`** | Extend extraction APIs as needed; remain **read-only** on PST |
| **`matter-core`** | Persist items, families, CAS, jobs, errors, audit, logical hash |
| **`ingest-purview`** | Optional: list discovered PSTs from a source; **do not** re-expand zips here |
| **`dedup-engine`** | Optional reuse of EML header formatting ideas; **do not** replace Desk `logical_hash` with Tier 2 |
| CLI/GUI | Optional thin smoke later; **not** DoD |
| **New crate (required)** | `crates/extract-pst` |

### 2.6 Desktop / threading rules

- Single-exe; never mutate source PST/Purview files.
- Extract is **CPU + IO heavy**. Public APIs are **sync** and **must** be called from a blocking worker (`std::thread`, rayon, `spawn_blocking`). Document in crate README (same as 0016).
- **0019** owns the process runner / pool; this track only documents the contract and may accept `cancel: Option<&dyn Fn() -> bool>`.
- Never hold one SQLite write transaction across an entire multi-GB PST parse — **batch commits** (plan §4.6).

---

## 3. In scope

### 3.1 Crate / workspace

1. Create **`crates/extract-pst`** library; workspace member.
2. Dependencies: `matter-core`, `pst-reader`, `camino`, `thiserror`, `serde`/`serde_json`, `chrono` (FILETIME → RFC3339). Prefer **hand-rolled length-prefixed / fixed field order** for native message serialization unless a dep is justified in review (bincode ok only if pinned and versioned in the blob header).
3. Extend **`pst-reader`** only for missing **read** surfaces required by this track (properties, **streaming** attachment data, open-from-reader if practical).
4. Extend **`matter-core` CAS** if needed: streaming put (`Read` → SHA-256 + write object) so multi-GB payloads never require a full `Vec<u8>` in RAM. Today only `put_bytes(&[u8])` exists — **streaming put is in scope for this track** when attachment size can exceed a small buffer.
5. No Tokio requirement in `extract-pst`.

### 3.2 PST open sources

Support at least:

| Mode | Behavior |
|---|---|
| **Filesystem** | `PstFile::open(path)` when path exists on the original package/FS path |
| **CAS whole-file** | Given inventory `native_sha256`, stream CAS object to a **matter-local** temp file (not OS `%TEMP%`), open, parse, delete when finished |

**Prefer:** if inventory row has a still-present package root path **and** the file exists, open from disk (avoid re-materializing multi-GB PST). Else fall back to CAS materialization.

#### 3.2.1 Matter-local workspace temp (required — no evidence in `%TEMP%`)

Spilling a full client PST into the OS temp directory is a **custody and leak risk**: `Drop` does not run on kill/power loss, leaving multi-GB unencrypted evidence under `%TEMP%` indefinitely.

**Rules:**

1. Materialize only under the matter root, e.g.  
   `<matter-root>/workspace/temp/`  
   (create `workspace/` + `temp/` as needed; document layout in matter-core / extract-pst README).
2. Use unique file names (job id + digest prefix + random suffix).
3. Best-effort delete on successful close **and** on error paths.
4. **Startup / open cleanup:** `Matter::open` / `Matter::create` (or an explicit `Matter::cleanup_workspace_temp()` called from both) **removes leftover files under `workspace/temp/`** so crash residue cannot accumulate. P0: recursive delete of temp contents. Optional later: overwrite-before-delete for secure wipe (not DoD).
5. **Do not** use `std::env::temp_dir()` for PST evidence materialization.

API should accept an explicit `PstLocate` / `PstOpenSpec` so callers are not guessing.

### 3.3 Target selection

Public entry points should support:

1. **`extract_pst_path(matter, source_id, pst_fs_or_logical_path, …)`** — operator points at a PST.
2. **`extract_pst_item(matter, source_id, inventory_item_id, …)`** — extract using 0016 inventory row (path + digest).
3. Optional helper: **`list_discovered_psts(matter, source_id)`** — items whose path looks like `.pst` and status is inventory-ish.

Create job kind **`extract_pst`** with stage **`pst_extract`**.

### 3.4 Message / attachment mapping

#### 3.4.1 Stable logical path convention

Under the package `source_id`:

```text
{pst_inventory_path}!/{folder_path}/{message_nid_hex}
{pst_inventory_path}!/{folder_path}/{message_nid_hex}/attach/{attach_index}_{safe_filename}
```

- `message_nid_hex` = lowercase hex of PST message NID (stable within that PST file content).
- Folder path uses `/` separators matching `pst-reader` `FolderInfo.path`.
- Resume skip key remains `(source_id, path)` via `item_by_source_path` (no unique index yet — app-level).

#### 3.4.2 Parent email item

| Field | Source |
|---|---|
| `path` | Convention above |
| `role` | `parent` |
| `file_category` | `email` |
| `mime_type` | `message/rfc822` or `application/vnd.ms-outlook` (document choice) |
| `message_id` | Normalized via `normalize_message_id` when present |
| `subject` | PidTagSubject |
| `from_addr` | Sender SMTP/email |
| `to_addrs_json` / `cc_addrs_json` / **`bcc_addrs_json`** | Prefer structured recipients; else parse Display* lists; **never invent BCC**; empty array if unknown |
| `sent_at` | ClientSubmitTime → RFC3339 UTC |
| `received_at` | Delivery time when available |
| `size_bytes` | Message size if known |
| `status` | `extracted` or `partial` |
| `text_sha256` | CAS of **full** body text used for logical hash (UTF-8) |
| `html_sha256` | Optional if HTML body extracted |
| `native_sha256` | See §3.5 |
| `logical_hash` + `logical_hash_version` | After children digests known — call **0017** helpers |
| `attachment_count` | Via family API / recompute |
| `extra_json` | e.g. `{ "pst_nid": "...", "folder": "...", "extract_tool": "extract-pst", "extract_version": "…" }` |

#### 3.4.3 Attachment items

| Field | Source |
|---|---|
| `role` | `attachment` |
| `parent_item_id` | Parent email item id |
| `family_id` | Same family |
| `file_category` | `attachment` (refine mime later) |
| `title` / path filename | Attach long filename |
| `size_bytes` | Attach size |
| `native_sha256` | CAS of **raw attach bytes** when readable (**streamed** — see §3.5.1) |
| `status` | `extracted` / `error` / `partial` |
| `logical_hash` | Optional non-email hash for attachment; parent email hash **must** include child natives |

If attachment bytes cannot be read: record `item_errors` (`stage=pst_extract`, code e.g. `attach_data_missing`); parent may be `partial`. **Never invent digests.**

#### 3.4.4 Family workflow (required order)

1. Insert/update parent shell (optional early insert).
2. Create `insert_family(FAMILY_KIND_EMAIL_ATTACHMENTS)`.
3. For each attachment: **stream** into CAS → insert child → `set_item_family_role(..., attachment, parent)`.
4. Build `EmailLogicalInput` including **bcc** (empty `Vec` if unknown) + attachment `(filename, size, native_sha256)`.
5. `compute_email_logical_hash` → `update_item` parent with hash + version + fields.
6. Set parent role `parent` + family.

Respect **family cohesion** rules from 0017.

### 3.5 Physical `native_sha256` (custody) — policy

Messages are not loose files. **Native identity must be stable for chain-of-custody.**

#### 3.5.1 Attachments: stream into CAS (no full-buffer OOM)

A PST attachment may be multi-GB (video, nested ZIP). **Forbidden for DoD:**

- `pst-reader` APIs that only return `Vec<u8>` for full attach payloads as the production path
- Calling `put_bytes` with an entire multi-GB buffer

**Required:**

1. `pst-reader` exposes attachment data as **`std::io::Read` + known/estimated size** (or chunk iterator over block reads) from LTP/NDB — not “load all then return.”
2. `matter-core` CAS gains **`put_read` / `put_reader`** (name flexible): hash while streaming to a temp object under `blobs/`, then atomic rename into the final CAS path (same collision policy as `put_bytes`).
3. Bounded buffers only (e.g. 64 KiB–1 MiB read loops). Optional hard cap `max_attachment_bytes` fails closed with `attach_too_large` rather than OOM.
4. Small-message bodies / small natives may still use `put_bytes` when size is known and under a documented threshold (e.g. 16 MiB).

#### 3.5.2 Email parent native: versioned `pst-native-message` (not synthetic EML)

| Object | Native bytes written to CAS |
|---|---|
| **Attachment** | Raw attachment payload stream (physical bytes from PST) |
| **Email parent** | **Required:** deterministic **`pst-native-message` v1** blob |

**Why not minimal EML as `native_sha256`:** Reconstructing RFC 5322 from MAPI is lossy and formatter-sensitive. Fixing a date/header encoding bug would **change** digests, break chain-of-custody, and invalidate prior reviews that keyed on native identity. That is unacceptable for foundational identity.

**Required native policy (document in README):**

1. Define a rigid, **version-tagged** serialization (`pst-native-message` / `v1` magic + version u32) over a **fixed, ordered set** of extracted MAPI-derived fields and raw property payloads where needed (message NID, key props, body text/html bytes, attachment digests/names — exact field list frozen in Phase 1 and golden-tested).
2. Prefer hand-rolled length-prefixed framing (same spirit as logical_hash) or a pinned binary codec **with the format version inside the blob**. Changing the format → bump **native format version** and leave old digests as historical; do not silently reformat v1.
3. `native_sha256` = CAS digest of those exact bytes.
4. Record `native_format` / version in `extra_json` (e.g. `"pst-native-message-v1"`).

**EML generation** is **out of scope for identity** here. Treat as **0040 ProductionExport** (or a later export helper). Do not use EML digests as `native_sha256` in 0018.

**Still forbidden:**

- Whole PST file digest as each message’s `native_sha256`
- Logical-hash preimage stored as “native”

### 3.6 Recipient / BCC extraction (defensibility)

0017 **requires BCC in the logical_hash preimage**. Extractor must:

1. Attempt to read DisplayBcc / structured recipient table if implemented.
2. If BCC unknown, store `[]` and document limitation — **do not** copy To into Bcc.
3. Always pass `bcc: Vec<String>` into `EmailLogicalInput` (empty allowed).
4. Tests: fixture or unit-level mapping where BCC present → `bcc_addrs_json` non-empty; hash differs from same fields without BCC (via `compute_email_logical_hash`).

Parsing DisplayTo-style semicolon lists is acceptable for P0 if documented as best-effort.

### 3.7 Body handling

1. Prefer **full** plain body for logical hash + `text_sha256` (extend reader to stop truncating at 4KB for the Desk path; CLI dedup can keep a preview API).
2. Normalize with `normalize_body` before hash; CAS stores UTF-8 bytes of the text used for hash (document consistency).
3. HTML optional; if both exist, plain body drives logical hash unless plain empty.
4. **Do not reimplement** logical-hash framing — 0017 already length-prefixes body/attachments so body text cannot spoof structure.

### 3.8 Checkpoints / resume (plan §4.6) — mid-folder guaranteed

Stage name: **`pst_extract`**.

A single Inbox can hold **100k+** messages. Checkpointing only at folder boundaries is **not** acceptable.

| Grain | Requirement |
|---|---|
| **Mandatory** | After every **`batch_size` messages** (default **500**), **including mid-folder** — yield, commit, write checkpoint, then continue the same folder |
| Also | After each folder completes (cursor advances folder + clears/updates last NID) |
| Cursor | `last_folder_path`, `last_message_nid` (or index within folder’s message_nids), `completed_count`, `pst_item_id` / open digest, `batch_size` |

`cursor_json` example:

```json
{
  "source_id": "src_…",
  "pst_path": "files.zip!/mail.pst",
  "pst_native_sha256": "…",
  "last_folder_path": "Root/Inbox",
  "last_message_nid": "0x2004",
  "folder_message_index": 40500,
  "completed_count": 41200,
  "batch_size": 500
}
```

**Walk / resume rules (required):**

1. **Correctness:** Skip any message path already `extracted` (and preferably non-null `logical_hash`) via `item_by_source_path` — inventory is authoritative for “already done.”
2. **Efficiency:** On resume, **do not** re-process completed messages in a 150k Inbox. Prefer:
   - retain ordered `message_nids` for the current folder and resume at `folder_message_index + 1`, **or**
   - skip NIDs ≤ cursor within the current folder before extracting again.
3. `pst_reader::folders()` returning a full `Vec` is ok for P0 structure discovery, but the **adapter walk loop** must process message_nids in batches of `batch_size` with checkpoints **before** finishing a huge folder.
4. Re-open same PST (path or matter-local CAS temp).
5. Cancel → job `Paused`; checkpoint durable; resume-capable.

**Batching:** commit SQLite every batch; never one-transaction the whole PST or whole folder of 100k messages.

### 3.9 Errors and partial success

| Situation | Behavior |
|---|---|
| Corrupt single message | `item_errors`; continue; parent skipped or `error` row optional |
| Unreadable attachment data | error on child; parent `partial` if email fields ok |
| ANSI / open failure | fail job with structured code; no silent empty success |
| Cancel mid-PST | checkpoint + pause |

Codes examples: `pst_open_failed`, `pst_ansi_rejected`, `message_props_failed`, `attach_data_missing`, `cas_put_failed`, `cancelled`.

### 3.10 Audit

Append at least:

| Action | When |
|---|---|
| `extract.start` | Job begins (path/digest/limits) |
| `extract.complete` | Success + counts |
| `extract.fail` | Fatal |

Avoid per-message audit spam (0017 policy). Tool version = `extract-pst` package version.

### 3.11 Blocking / cancel API sketch

```rust
pub struct ExtractLimits {
    pub batch_size: u64,                    // default 500; enforced mid-folder
    pub max_messages: Option<u64>,          // test/safety cap
    pub max_attachment_bytes: Option<u64>,  // fail closed if exceeded
    pub max_in_memory_put_bytes: u64,       // below this, put_bytes ok; above → stream
}

pub struct ExtractSummary {
    pub source_id: String,
    pub job_id: String,
    pub messages_ok: u64,
    pub messages_err: u64,
    pub attachments_ok: u64,
    pub attachments_err: u64,
    pub completed: bool,
    pub cancelled: bool,
}

/// Blocking. Call from worker thread only.
pub fn extract_pst_item(
    matter: &Matter,
    source_id: &str,
    pst_item_id: &str,
    limits: &ExtractLimits,
    cancel: Option<&dyn Fn() -> bool>,
) -> Result<ExtractSummary>;

pub fn resume_extract(
    matter: &Matter,
    source_id: &str,
    job_id: &str,
    limits: &ExtractLimits,
    cancel: Option<&dyn Fn() -> bool>,
) -> Result<ExtractSummary>;
```

### 3.12 Tests (required)

1. **Happy path:** fixture Unicode PST (`fixtures/*.pst`) → messages as items, families for multi-attach if present, `logical_hash` set, audit chain verifies.
2. **Resume mid-folder:** `batch_size=1` (or small N); cancel after first message(s) **in a multi-message folder** → resume completes remaining without duplicate paths; cursor/`last_message_nid` advanced.
3. **Partial:** force one bad NID or inject error path → other messages extracted; errors recorded.
4. **Open from CAS:** put fixture bytes in matter CAS, extract without original path; temp under `workspace/temp/` only.
5. **Temp cleanup:** leave a fake file under `workspace/temp/`, call `Matter::open` (or cleanup helper) → file gone.
6. **Streaming CAS:** unit/integration that `put_reader` (or equivalent) hashes known stream without requiring full buffer API for large simulated reads (can use multi-chunk reader).
7. **ANSI / bad file:** structured failure.
8. **BCC / recipient unit tests** if fixture weak — property/unit on mapping helpers.
9. **Logical hash integration:** same fields → same hash as direct `compute_email_logical_hash` call.
10. **Native stability:** golden `pst-native-message-v1` bytes for a fixed synthetic field set → stable digest.
11. Workspace gate + **`ledgerful verify`**.

Use **synthetic/small fixtures only** in git (existing Aspose/sample PSTs). No client mail.

### 3.13 Docs

- `crates/extract-pst/README.md`: blocking warning, path convention, **pst-native-message-v1** (not EML), streaming attach, matter-local temp, mid-folder checkpoints, open-from-CAS, out of scope.
- matter-core README: `workspace/temp/` cleanup + streaming CAS if added.
- Extend `pst-reader` docs/comments for any new streaming APIs.
- Root `ARCHITECTURE.md` / `README.md` crate map.
- `review.md` on completion.

### 3.14 Optional (not DoD)

- CLI `extract` subcommand.
- **EML export** for human download (prefer **0040** production path — not native identity).
- Recipient table (MAPI) vs Display* only.
- Parallel folder extract (single-threaded P0 is fine).
- Fuzz of property decoders (nice; fixture coverage minimum).
- Secure multi-pass wipe of temp (beyond delete-on-open).

---

## 4. Out of scope (do NOT do here)

| Deferred | Work |
|---|---|
| **0016** (done) | ZIP expand / package detect |
| **0017** (done) | Schema / logical hash pure functions / preimage framing |
| **0019** | Generic job runner, progress channels, `spawn_blocking` pool ownership |
| **0020** | Desk UI |
| **0021** | Matter-wide dedupe job |
| **0022** | Threading / conversation_id |
| **0033+** | Office/PDF as first-class extractors |
| **0040** | Production EML/load-file export (not used for `native_sha256`) |
| — | Writing PST; mutating evidence; 7z; cloud AI |
| — | Replacing or redesigning `logical_hash` v1 (unless proven regression) |
| — | Using synthetic EML digests as native custody identity |

---

## 5. Preconditions & dependencies

- **P1:** **0016** Completed — inventory PSTs + CAS.
- **P2:** **0017** Completed — schema v2, family cohesion, `logical_hash` v1 + BCC + length-prefix framing.
- **P3:** Fixtures under `fixtures/*.pst` usable offline.
- **P4:** `cargo test -p matter-core` and `cargo test -p ingest-purview` green before/after.
- *Verified research snapshot:*
  - `MessageProperties` truncates body to 4KB and has no BCC fields.
  - `read_attachment_metadata` does not return bytes.
  - `PstFile::open` is path-only today.
  - 0016 stores whole PST in CAS with status `discovered`.
  - 0017 `LOGICAL_HASH_VERSION == 1`, `SCHEMA_VERSION == 2`.

---

## 6. Risks

| Risk | Mitigation |
|---|---|
| Incomplete MAPI coverage | Honest partial; Display* fallback; codes; expand reader incrementally |
| BCC often missing in PST props | Empty bcc + docs; never fabricate; tests when data exists |
| **Huge attachments OOM** | **Streaming Read → CAS; no full `Vec` production path; size caps** |
| **Native identity churn (EML formatter)** | **`pst-native-message-v1` only; EML deferred to 0040** |
| **%TEMP% evidence leaks after crash** | **`workspace/temp/` under matter; wipe on Matter open** |
| Multi-GB PST CAS materialize | Prefer FS path; stream CAS→matter temp; cleanup |
| **150k-message folder resume cost** | **Mid-folder batch_size checkpoints + last_message_nid / index** |
| Duplicate items on resume | Skip by `(source_id, path)`; stable NID paths |
| Transaction too large | Batch every N messages mid-folder |
| Confuse Tier 2 / logical / native | Explicit policies in README |
| Freezing UI | Blocking-thread contract |
| Accidental rework of 0017 hash | Call `compute_email_logical_hash` only; no parallel preimage |

---

## 7. Definition of Done

Complete only when ALL hold:

- [ ] **DoD-1 — Crate:** `crates/extract-pst` is a workspace member; `cargo test -p extract-pst` runs.
- [ ] **DoD-2 — Extract:** Fixture PST → parent items + attachment children (when present), families linked, statuses `extracted`/`partial`.
- [ ] **DoD-3 — Identity:** Parent has `message_id` when present, `logical_hash` + `logical_hash_version=1`; parent `native_sha256` is **`pst-native-message-v1`** (not EML); attachment `native_sha256` when stream readable; policies documented.
- [ ] **DoD-4 — Recipients:** To/Cc/Bcc fields populated best-effort; **BCC never fabricated**; `EmailLogicalInput.bcc` always supplied (empty or real).
- [ ] **DoD-5 — Resume mid-folder:** Checkpoint every `batch_size` **inside** large folders; interrupt + resume does not duplicate paths; cursor/`last_message_nid` (or index) respected.
- [ ] **DoD-6 — CAS open + temp hygiene:** Extract from inventory digest without FS path; materialize only under `workspace/temp/`; leftover temp cleaned on matter open.
- [ ] **DoD-7 — Streaming attach path:** Production attach→CAS path does not require full-payload `Vec<u8>`; streaming put exists and is tested.
- [ ] **DoD-8 — Errors:** Per-message/attachment failures recorded; job can still succeed partially or fail honestly.
- [ ] **DoD-9 — Audit + docs:** start/complete|fail audit; README (blocking, native v1, streaming, temp, mid-folder); ARCHITECTURE/README note.
- [ ] **DoD-10 — Workspace gate:** fmt, clippy `-D warnings`, relevant tests, **`ledgerful verify`** (required).
- [ ] **DoD-11 — Recorded:** `review.md`; `../conductor.md` → **Completed**; ledger TX (`FEATURE`).

---

## 8. Verification commands (reference)

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p pst-reader
cargo test -p matter-core
cargo test -p extract-pst
cargo test --workspace
ledgerful verify
```

---

## 9. Acceptance narrative

An implementer (or test) can:

1. Ingest or register a fixture PST into a matter (0016 path or direct CAS put + inventory row).
2. Run `extract_pst_item` on a **blocking** worker.
3. Observe folder/messages as Normalized Items with families, **`pst-native-message-v1`** natives (not EML digests), streamed attachment CAS digests, `logical_hash`, and audit.
4. Cancel **mid-folder** after a small batch; resume without duplicating paths and without re-doing completed NIDs.
5. Open the same PST only from CAS via **`workspace/temp/`** (not `%TEMP%`); leftover temp cleaned on matter re-open.
6. Hand **0021** items queryable by `logical_hash` / `message_id`.
