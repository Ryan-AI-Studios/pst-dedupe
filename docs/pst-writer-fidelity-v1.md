# pst-writer — Production Unicode PST Writer v1 (Track 0068)

Scope: `pst_writer::write_unicode_pst` (crate `pst-writer`, module `production`).
This is the **production write path**. The pre-existing `write_pst_from_emls`
fixture entrypoint (module root of `pst-writer`) is unchanged and out of scope
for the guarantees below — it still truncates bodies to 2000 chars and is
single-block only; keep using it only for existing fixture callers.

## Fidelity matrix

| Feature | v1 | Notes |
|---|---|---|
| Unicode, unencrypted PST | Yes | `wVer = 23`, `bCryptMethod = 0`. ANSI PST write: never. |
| Full plain body | Yes (XBLOCK/XXBLOCK) | No silent truncation at any length. |
| HTML body | Yes, if present | Stored as `PtypBinary` (raw bytes); `pst-reader`'s `ExtractedMessage.body_html` resolves it via its string→binary fallback. |
| Plain-only native body | `PidTagNativeBody = 1` (Plain) | Forced when no HTML is written. |
| HTML native body | `PidTagNativeBody = 3` (HTML) | Set whenever HTML is written (with or without plain fallback). |
| No body written | NativeBody/EditorFormat/Codepage omitted | Never invents a body; `body_unavailable = true` always yields no body regardless of `body_plain`/`body_html` content. |
| Body fidelity reporting | `WritePstReport.messages_with_incomplete_body` / `.messages_with_unavailable_body` | Counts of written messages whose source `WriteMessage.body_incomplete`/`.body_unavailable` flag was set — see below. |
| `PidTagMessageSize` | Computed | See formula below — never copied from a source-declared size. |
| IPM_SUBTREE hierarchy | Yes | `Root → IPM_SUBTREE → <folder>` (see below); store carries `PidTagIpmSubtreeEntryId`. |
| IPM_SUBTREE required initialization | Yes | `PidTagDisplayName = "Top of Personal Folders"`, `PidTagContentCount = 1`, `PidTagContentUnreadCount = 0`, `PidTagSubfolders = true` — verified MS-PST requirement (round 9); see below. |
| Deleted Items folder | Yes, empty | Real folder object (PC + hierarchy/contents/assoc-contents TCs), child of IPM_SUBTREE; referenced by the store's `PidTagIpmWastebasketEntryId`. v1 never invents deleted-items content. |
| Search Root folder | Yes, empty | Real folder object (`NID_TYPE_SEARCH_FOLDER`), not a hierarchy child; referenced by the store's `PidTagFinderEntryId`. v1 never implements search semantics. |
| Fixed MS-PST template-object tables | Yes, always empty | Hierarchy/Contents/AssocContents/SearchContents Table Templates at fixed NIDs `0x60D`/`0x60E`/`0x60F`/`0x610`; zero data rows, correct column schema. **0098:** `alloc_nid` skips nidIndex `0x30`/`0x33`/`0x34` so a user folder's hierarchy/contents/assoc NIDs cannot collide with these templates (or `0x671`/`0x692`). Duplicate NBT NIDs fail closed. |
| Associated-contents (FAI) table | Yes, empty | Root, IPM_SUBTREE, `<folder>`, Deleted Items, and Search Root each get an empty associated-contents TC (NID suffix `0x0F`) alongside their PC/hierarchy/contents TCs — MS-PST §2.4.2 completeness; see below. No FAI items are ever written in v1. |
| Attachments | **Yes (v1.1 / 0069)** | By-value file attaches + attachment table + XBLOCK; see **v1.1** section below. |
| Folder path preservation | **Yes (v1.1 / 0069)** | Default `PreservePaths` under IPM_SUBTREE; residual `Unique Mail`. |
| Multi-source prefixes | **Yes (v1.1 / 0069)** | Unique stem labels (`archive` / `archive (2)`). |
| Multi-GB streaming write | **Yes (v1.2 / 0070)** | `write_unicode_pst_streaming`: AMap-aware layout, chunked attach stream, progress + `stop_and_finalize`, SHA-256/MD5 report; see **Scale** section. |
| Encrypted / Permute output | **No** | Residual; unencrypted only. |
| ANSI PST | **No** | Never. |
| Recipient table | **Yes (0082 / 0100 Strategy A)** | Store template NID **`0x692`** (zero rows, **14 MUST columns** per MS-PST Recipient Table Template). Every written message gets a per-message recipient TC subnode (may be **zero rows** when source had none / unreadable — empty TC still present: `hnidRows = 0`, `bid_sub = 0`). One row per **included** recipient; optional extra column `PidTagSmtpAddress` (`0x39FE`) when known. Structural columns synthesized when source omits them (`ObjectType=6`, Responsibility, RecordKey/EntryId/SearchKey patterns, etc.). **0100 Strategy A:** all included rows (To→Cc→Bcc **order** only; no cap). Row matrix is a subnode (`hnidRows` = NID) packed with §2.3.4.4 RowsPerBlock (live width 56 → 146). Recipient-table node uses multi-page HN (HNHDR + HNPAGEHDR). Per-row strings &gt; `MAX_HEAP_VALUE_SIZE` (2048) divert to a cell NID. Production does not emit `RECIPIENT_TC_TRUNCATED`. Display* on the message PC stay full. Residuals: attachment-table TC (`D-0093-attachment-tc-page`); HNBITMAPHDR (`D-0100-hn-bitmap-hdr`). |
| Named-prop set beyond the store stub | **Allowlisted (0092)** | When used: real NPMAP (GUID/entry/string + hash buckets, BucketCount=251) for `PSETID_Attachment` allowlist (`AttachmentProviderType` MUST when known; Url/PermissionType MAY if present). Empty stub when unused. Full encyclopedia still out of scope (**D-0084-cloud-named-prop-write** closed for allowlisted write). |
| RTF | **No** | v1 never writes `PidTagRtfCompressed` or any RTF-native hint — there is nothing RTF-related to clear because nothing RTF-related is ever produced. |
| `PidTagMessageFlags` | `MSGFLAG_READ` (0x1); `\| MSGFLAG_HASATTACH` (0x10) when ≥1 attach written | Paperclip + read default (0069). |
| `PidTagDisplayTo` | Yes, when present | Written from source display To string. |
| `PidTagDisplayCc` | **Yes (0080 §3.11)** | Written when present; previously silently dropped. |
| `PidTagDisplayBcc` | **Opt-in (0082)** | Default **OFF** (`include_bcc_recipients: false`): omit Bcc TC rows and `PidTagDisplayBcc` (disclosure policy on consolidated unique-PST). Opt-in via `WritePstOpts::include_bcc_recipients` / CLI `--include-bcc-recipients` writes Bcc rows + display BCC when source provided them. Export ledger column `bcc_suppressed` records omissions. |
| `PidTagCreationTime` / `PidTagLastModificationTime` | Set to `submit_time` when present; omitted otherwise | This is a synthetically-written export item, not a live mailbox object, so `submit_time` is a defensible stand-in for both when no better source exists. Never invented — omitted entirely when `submit_time` is `None`. |

## v1.1 — Attachments + folder fidelity (Track 0069)

Closes **D-0068-04** (attachment table + attach objects). Builds on the v1 store
shape above without regressing XBLOCK bodies, IPM special folders, or safety.

### Attachments

| Behavior | Detail |
|---|---|
| **By-value (`ATTACH_BY_VALUE` = 1)** | `PidTagAttachDataBinary` heap-inline when small; **subnode + XBLOCK** when larger than one heap page. |
| **Attachment table** | PST-level **template** at NBT NID `0x671` (zero rows, full column schema). Per-message subnode NID `0x671` TC with one row per successfully written attach: AttachSize, AttachFilename (HNID), AttachMethod, RenderingPosition (`0xFFFFFFFF`), LtpRowId (= attach NID), LtpRowVer; **RowIndex BTH** (`hidRowIndex`, key=attach NID, value=0-based index). |
| **Attach object NIDs** | Type `0x05` (`NID_TYPE_ATTACHMENT`) as **message subnodes only** (never top-level NBT). |
| **HasAttachments** | `true` only when ≥1 attach was actually written. |
| **MessageFlags** | Always `MSGFLAG_READ` (0x1). OR `MSGFLAG_HASATTACH` (0x10) when attaches written. |
| **MessageSize** | Computed from bytes written (never source size). **Inline** attach binary is inside the attach PC → count only attach PC length; **subnode** diversion adds PC + raw bytes (same rule as body inline vs diversion). |
| **Soft fail** | Missing data (`data: None` and no stream) / unsupported method → skip that attach, `attachments_failed++`; message still written. **`data: Some(vec![])` is a valid zero-byte by-value attach** (not a soft fail). |
| **Stream source** | Optional [`AttachStreamSource`] via `write_unicode_pst_with_streams` / `write_unicode_pst_streaming`. Stream is consulted **only if** `data: None`. Prefer **`open_attach_stream`** → chunked `MAX_BLOCK_DATA` (8176) leaf chain without a full multi-GB attach `Vec` (**D-0069-stream-buffer closed in 0070**). Default stream impl wraps `open_attach` in `AttachRead::from_vec` for compat. Mid-stream I/O error soft-fails the attach. |
| **parents_only** | `WritePstOpts::parents_only` empties attach list; `attachments_omitted_by_policy++`. |
| **Embedded (`ATTACH_EMBEDDED_MSG` = 5)** | Nested message PC under attach **subnode** when `WriteAttachment.embedded_message` present; method=5; size reflects nested; **never** invent by-value file bytes. Writes **`PidTagAttachDataObject` as PtypObject `0x3701`/`0x000D`** (8-byte heap `{Nid, ulSize}`) — never non-empty PtypBinary on that tag. Missing nested → `embedded_unparsed++` + `attachments_failed++` + fidelity event. Extract depth/budget → `ATTACH_DEPTH_LIMIT` via `embedded_depth_limited`. |
| **Depth cap** | `max_embedded_depth` default **3**, clamp `[1, 8]`. unique-pst exposes this as `--max-embedded-depth` (0101; clap rejects outside 1–8; writer clamp unchanged). Deeper branches halt; `embedded_depth_limit_hits++` + fidelity event (DoD-8 surface — not a MAPI property on the item). |
| **CloudLink (classified)** | Write **metadata/pointer row** (classic tags: method, long pathname/URL when known, optional Pathname 0x3708, filename when known — **no invented name**, **no** `PidTagAttachDataBinary`) plus allowlisted named props when `NamedPropWritePlan` includes them (0092). Emit fail-severity `ATTACH_CLOUD_LINK` (payload not collected offline). Network hydration never. |
| **Body-inline cloud URLs (0085)** | **Not** an Attachment Table fidelity surface. unique-pst report pack detects document-shaped body URLs offline (`export_body_cloud_links.csv`); does **not** invent attach rows or change writer attach behavior. |
| **Non-cloud OLE / ref methods** | Still **omit** + fail `ATTACH_METHOD_UNSUPPORTED` (method ∉ {1, 5} and not CloudLink-classified). |

`from_canonical_message` **maps** `CanonicalAttachment` metadata (and small `data`
when present) plus `locus.folder_path` / `locus.source_path`. The second return
value is reserved for unmappable attaches (0 today).

### Folder layout

Default policy: `FolderLayoutPolicy::PreservePaths { multi_source_prefix: true }`.

Operator-facing contract (sentinels, lazy residual, pre-seed): [`unique-pst-export.md`](unique-pst-export.md) § Folder tree contract (0095).

```text
IPM_SUBTREE ("Top of Personal Folders")
  ├── <source_prefix>/?     # when ≥2 distinct sources + multi_source_prefix
  │     └── path segments after leading IPM/root alias strip
  ├── Unique Mail/          # residual only when allocated (lazy in preserve)
  └── Deleted Items/        # special folder (0068); may hold winners
```

| Rule | Detail |
|---|---|
| Case routing | Case-insensitive segment match; **first-seen display name wins**. |
| Leading aliases (0095) | Strip consecutive leading `root` / `top of personal folders` / `top of information store` / `top of outlook data file` / `ipm_subtree` only; stop at first non-alias. |
| Multi-source prefixes | Sanitized file stem; **case-folded uniqueness** so `Archive.pst`/`archive.pst` never merge; collisions → `archive`, `archive (2)`, …. With `WritePstOpts.known_source_paths` (≥2 sources) prefixes are stable from message 1 (**D-0070 closed** in 0095). Bare writer without pre-seed still discovers from stream order. |
| Single-source | No source prefix. |
| Unique Mail | Preserve: allocate on first residual/unparseable path only. Flat: eager display-name folder. |
| Sanitize | Strip `<>:"/\|?*`, collapse `..`/empty, max 32 segments → residual. Sanitized segments or `..` / over-depth → `folder_paths_degraded++`; residual routing also increments `folder_paths_residual` when path missing/empty/invalid. |
| Flat policy | `FolderLayoutPolicy::Flat { folder_display_name }` — all messages in one folder (0068 behavior). |
| Folder object | Every folder still four-part: PC + hierarchy + contents + assoc-contents. |

### Report counters + per-attachment fidelity events (0069 / **0073**)

`WritePstReport` adds aggregate counters: `attachments_written`, `attachments_failed`,
`attachments_omitted_by_policy`, `folders_created`, `embedded_messages_written`,
`embedded_depth_limit_hits`, **`embedded_unparsed`**, **`folder_paths_residual`**,
**`folder_paths_degraded`**.

Plus **`attachment_fidelity_events: Vec<AttachmentFidelityEvent>`** — per-attachment
honesty surface (not stored as MAPI properties). **0073** expands locus + reason
taxonomy so every former silent `attachments_failed++` path emits an event.

| Field | Meaning |
|---|---|
| `message_subject` | Display only (not a primary key) |
| `attach_filename` | Attachment filename as supplied on the DTO |
| `kind` | `AttachmentFidelityKind` — stable `as_code()` → `SCREAMING_SNAKE` |
| `source_path` / `folder_path` | Locus (empty if unknown) |
| `msg_nid` / `attach_nid` / `attach_index` | Joinable identity |
| `size` / `attach_method` | Best-effort metadata (`attach_method` = −1 if unknown) |
| `severity` | `Fail` (counts) or `Info` (policy omit; not in `attachments_failed`) |

**Reason codes (`kind.as_code()`):**

| Code | Severity | When |
|---|---|---|
| `ATTACH_METHOD_UNSUPPORTED` | fail | method ∉ {1, 5} and not CloudLink-classified → attach omitted |
| `ATTACH_CLOUD_LINK` | fail | CloudLink classified → pointer/metadata row written (no binary); payload not collected offline (0084) |
| `ATTACH_STREAM_OPEN_FAILED` | fail | resolve/open payload None or open err |
| `ATTACH_STREAM_READ_FAILED` | fail | mid-stream I/O while writing chain |
| `ATTACH_STREAM_CRC` / `ATTACH_BLOCK_NOT_FOUND` / `ATTACH_DATA_TRUNCATED` / `ATTACH_SIZE_CAP` | fail | reserved when distinguishable |
| `ATTACH_DEPTH_LIMIT` | fail | was `DepthLimitExceeded` |
| `ATTACH_EMBEDDED_UNPARSED` | fail | method-5 without nested message |
| `ATTACH_META_FAILED` | fail | materialize meta (when surfaced) |
| `ATTACH_OMITTED_BY_POLICY` | **info** | `parents_only` — does **not** increment `attachments_failed` |
| `ATTACH_UNKNOWN` | fail | last resort |
| `ATTACH_LEDGER_TRUNCATED` | info | CLI CSV row-cap marker (not a writer fail) |

Optional **`AttachEventSink`** on `write_unicode_pst_streaming` streams events to
callers (unique-pst mpsc → background CSV). Events always accumulate in the
report `Vec` for tests. Invariant: fail-severity event count == `attachments_failed`.

### Tests

- Regression: `crates/pst-writer/tests/writer_v1.rs` (0068 matrix).
- Fidelity: `crates/pst-writer/tests/writer_fidelity.rs` (0069 matrix §9 cases 1–15 + review fixes: attach template, per-message TC/RowIndex, degraded path, embedded_unparsed, case-differing multi-source prefixes, fidelity events).

### Still out of scope / residual

| Item | Owner |
|---|---|
| Multi-GB whole-file streaming | **Closed in 0070** (engine path; operator multi-GB residual) |
| One-attach full buffer without chunked stream | **Closed in 0070** (`open_attach_stream` + `write_data_chain_from_reader`) |
| `unique-pst` CLI + multi-volume product UX | **0071** (uses 0070 physical size / stop / hashes) |
| scanpst / Outlook operator proof | D-0068-02 (carry) — recommend on multi-GB operator run |
| Cloud attach network hydration / download | never in-scope offline; residual D-0067-cloud if ever reconsidered |
| Full named-prop encyclopedia / arbitrary NPMAP clone | residual after **0092** (allowlisted ProviderType/Url/Permission write closed **D-0084-cloud-named-prop-write**) |
| `PidTagAttachDataObject` (PtypObject) on embeds | **Yes (0094)** — `PcValue::Object`; reader resolves via `0x3701` first, subnode-scan fallback for 0069-era files |
| Per-folder contents-table RowIndex BTH | not required this track (attach table only) |
| Eager spill of all leaf block `Vec`s from `Layout` | **Closed in 0070 P1** — `EagerWriteCtx` spills leaves (`on_disk=true`); residual RAM is small internal blocks (XBLOCK/PC heaps) only |
| Full DTO pre-collect on streaming path | **Closed in 0070** — `IncrementalFolderPlan` one-pass consume; no `Vec<WriteMessage>` materialization by the writer |
| Multi-source prefix streaming residual | **Closed in 0095** — `WritePstOpts.known_source_paths` pre-seeds prefixes from message 1 (≥2 sources). Bare writer callers that omit pre-seed still discover from stream order. Fat in-memory bodies on caller DTOs remain the caller's responsibility |

## Scale — multi-GB streaming (Track 0070)

Architecture: **Hybrid / Approach C** — thin metadata + folder plan in RAM; bodies/attaches stream (or drop after write); AMap-aware offset placement; same-dir temp + rename-only finalize.

### AMap-aware allocation (MS-PST)

| Constant | Value | Meaning |
|---|---|---|
| `AMAP_FIRST_OFFSET` | `0x4400` (17408) | Absolute file offset of the first Allocation Map page |
| `AMAP_INTERVAL` | `253952` (`0x3E000`) | Subsequent AMap pages every this many bytes |

Data blocks and B-Tree pages **never land on** AMap page slots. When sequential placement would cross/land on a slot, the allocator reserves the AMap page at that absolute offset and resumes after it. AMap content is 496 bytes of `0xFF` free bits (v1 free accounting approximate). Header `ibAMapLast` is the last AMap offset.

### Memory model (documented peak bounds)

```text
O(N × thin folder plan + message NIDs)   // incremental plan; no full DTO collect
+ O(1) × current WriteMessage in flight (caller may still pass fat bodies)
+ O(1) × STREAM_CHUNK_SIZE (= MAX_BLOCK_DATA = 8176) for attach stream reads
+ O(1) leaf spill buffer (eager on_disk; Layout holds bid/offset/len only)
+ O(BBT/NBT page set + small XBLOCK/PC heaps at finalize)
```

**Forbidden on multi-GB path:** one full multi-GB attach `Vec`; writer pre-collect of all `WriteMessage` DTOs; holding all message bodies after they have been written; retaining all leaf block payloads in `Layout` until finalize.

**Honesty:** if the **caller** already holds a `Vec<WriteMessage>` with fat bodies, that RAM is the caller's. The streaming writer does not force lazy iterators to materialize. With `known_source_paths` pre-seed (≥2 sources; unique-pst default), prefixes match collect-all from message 1 (**D-0070 closed**). Unseeded direct-writer callers may still see stream-order prefix discovery.

**`WriteProgress.current_physical_size` during `WritingMessages`:** true cumulative size of the same-dir temp (eager write cursor / file metadata), not a layout estimate before offsets exist.

### Progress + volume split hooks (for 0071)

| API | Role |
|---|---|
| `WriteProgress.current_physical_size` | True cumulative temp size during WritingMessages (eager spill) and after finalize flush — not payload sum or pre-offset layout estimate |
| `WriteStage` | `Planning` / `WritingMessages` / `FinalizingNdb` / `Renaming` |
| `WriteProgressSink::should_stop_and_finalize` | After each fully written message only |
| `WritePstReport.finalized_early` | Partial volume; exact `messages_written` |

### Inline export hash

After all seeks complete and before rename, the complete temp file is hashed:

- `WritePstReport.sha256_hex` — SHA-256 lowercase hex
- `WritePstReport.md5_hex` — MD5 lowercase hex (legacy load-file)

Strategy: full-file hash of finalized temp (header/BBT/NBT/AMaps already written). Matches on-disk bytes after rename.

### Same-directory temp

Temp is always a sibling of the output path (`temp_sibling_path`). **Rename only** — no multi-GB cross-volume copy fallback. Fail early if same-dir create fails.

### Outlook size guidance (product honesty)

| Guidance | Detail |
|---|---|
| Interactive Outlook comfort | Industry often cites **~10 GB** per PST as painful (slow open/search); not a hard engine limit |
| Engine export max | Multi-GB without OOM is the goal; wall-clock/disk bound |
| Multi-volume CLI | **0071** — use physical size + stop_and_finalize + hashes |

### Tests

- Scale: `crates/pst-writer/tests/writer_streaming.rs` (AMap boundary, chunked attach, stop, hash, same-dir temp, CI ~16 MiB stream stress).
- Regression: `writer_v1` + `writer_fidelity` remain green.

## Hierarchy (§3.2)

```text
Header (Unicode, crypt=none)
  Message store (PC: PidTagDisplayName="Personal Folders", PidTagIpmSubtreeEntryId,
                 PidTagIpmWastebasketEntryId, PidTagFinderEntryId)
  Root folder (NID_ROOT_FOLDER)
    └── IPM_SUBTREE            (allocated NID; PidTagDisplayName="Top of Personal Folders";
                                 PidTagContentCount=1, PidTagContentUnreadCount=0,
                                 PidTagSubfolders=true)
          ├── <folder>         (default display name "Unique Mail"; configurable via
          │                     WritePstOpts::folder_display_name)
          │     └── Message 1..N
          └── Deleted Items    (always empty; referenced by PidTagIpmWastebasketEntryId)

Search Root                    (NID_TYPE_SEARCH_FOLDER; NOT a hierarchy child of IPM_SUBTREE;
                                 always empty; referenced by PidTagFinderEntryId)

Fixed template objects (NID_HIERARCHY_TABLE_TEMPLATE 0x60D,
  NID_CONTENTS_TABLE_TEMPLATE 0x60E, NID_ASSOC_CONTENTS_TABLE_TEMPLATE 0x60F,
  NID_SEARCH_CONTENTS_TABLE_TEMPLATE 0x610,
  NID_ATTACHMENT_TABLE_TEMPLATE 0x671,
  NID_RECIPIENT_TABLE_TEMPLATE 0x692) — always zero data rows
```

Root's own contents table is always empty — every message lives under
`<folder>`, which is a child of IPM_SUBTREE, never a direct child of root.

### Associated-contents (FAI) table — MS-PST §2.4.2 completeness (round-6 P1 finding, Item 2)

Per MS-PST §2.4.2, a complete Folder object is four sub-objects: the folder's
own PC, a Hierarchy Table, a Contents Table, and an **Associated Contents
Table** (a.k.a. FAI — Folder Associated Information), even when the latter is
empty. A round-6 cross-model review (codex) correctly identified that v1
originally gave each of Root, IPM_SUBTREE, and `<folder>` a PC + hierarchy TC
+ contents TC but no associated-contents TC — an incomplete Folder object by
the letter of §2.4.2, independent of any attachment/folder-tree scope
question. This was fixed: each of the three folders this track already
creates now also gets an empty associated-contents TC, using the exact same
`build_tc_inline_checked` empty-TC pattern already used for the (also always
empty in v1) hierarchy tables.

NID: the associated-contents table for a folder with NID `N` is `(N & !0x1F)
| 0x0F` — the same fixed-suffix scheme this writer already uses for hierarchy
(`| 0x0D`) and contents (`| 0x0E`). `0x0F` was not guessed: it is
cross-checked against this repo's own canonical NID-type numbering in
`pst_reader::ndb::nid::NodeId::associated_contents_table()` (`(self.0 & !0x1F)
| 0x0F`) and `NidType::AssocContentsTable`, both already present in
`crates/pst-reader/src/ndb/nid.rs` before this change. No new folder objects
are created by this fix — it only completes the definition of the three
folders v1 already writes. See
`crates/pst-writer/tests/writer_v1.rs::all_three_folders_have_readable_empty_associated_contents_table`,
which opens the written PST, resolves each folder's associated-contents NID
via `NodeId::associated_contents_table()`, loads it with
`pst_reader::ltp::tc::TableContext::load`, and asserts `row_count() == 0`.

### `PidTagIpmSubtreeEntryId` / `PidTagRecordKey` design (review fold #2; round-5 finding Part A)

`pst-reader` does not parse or resolve MAPI EntryIDs at all (it walks folders by
NID directly), and Outlook / `scanpst.exe` were not available in this
environment to independently verify EntryID acceptance. The EntryID written is
a documented, best-effort MS-OXCDATA-shaped 24-byte structure:

```text
abFlags     (4 bytes)  = 0x00000000
ProviderUID (16 bytes) = the store's own PidTagRecordKey (see below)
NID         (4 bytes)  = IPM_SUBTREE folder's NID, little-endian
```

The message store's own PC also carries **`PidTagRecordKey`** (MAPI tag
`0x0FF9`, `PtypBinary`) — a 16-byte value generated once per write by
`derive_store_record_key()` (`crates/pst-writer/src/production.rs`, track
**0087**) and reused, byte-for-byte, as the EntryID's ProviderUID above. This
closes a round-5 cross-model review finding: earlier v1 wrote no
`PidTagRecordKey` at all and hardcoded the EntryID's ProviderUID to an
arbitrary all-zero placeholder. A store-internal EntryID's provider UID is
conventionally the store's own unique record key, not an arbitrary value —
the fix makes the EntryID genuinely self-consistent and identifies this
specific store, rather than pointing at a degenerate zero placeholder.

**Default mode (0087):** domain-separated SHA-256 over a length-prefixed
preimage (algo v1 + `volume_index` + message count + content fingerprint from
ordered MID/subject/submit/folder). **No** wall-clock, PID, or destination
path in the default preimage — same logical winners ⇒ same RecordKey across
re-runs and dest paths. Optional job-global `store_key_material` rebinds each
volume key to the whole export. Ephemeral (time+pid) is an opts-only escape
hatch. Full volume-file `sha256_hex` match remains best-effort (B-tree/layout);
see `docs/unique-pst-export.md` CoC section and D-0079-deterministic-key
(closed).

This is **still not** independently verified against a real Outlook-opened
PST — flagged as a residual for operator scanpst/Outlook evidence (spec
§3.9-7/8); the "not independently checked" framing from the prior all-zero
placeholder no longer applies to *why* the ProviderUID has the value it does
(that question is now answered: it matches the store's RecordKey), only to
the fact that Outlook/scanpst haven't independently exercised it yet. The
synthetic test suite verifies: the property round-trips as 24 raw bytes and
the embedded NID matches the actual IPM_SUBTREE folder's NID (see
`crates/pst-writer/tests/writer_v1.rs::hierarchy_places_unique_mail_under_ipm_subtree_with_store_entryid`);
`PidTagRecordKey` is present, 16 bytes, and non-zero, and equals the EntryID's
ProviderUID bytes exactly (see
`store_record_key_present_nonzero_and_matches_entry_id_provider_uid`); same
content on different paths yields identical keys; different content or
`volume_index` yields different keys (see
`store_record_key_differs_across_separate_writes` and volume-index tests).

### `PidTagIpmWastebasketEntryId` / `PidTagFinderEntryId` — implemented (round 9; supersedes the round-5/6 decline)

Rounds 5–8 of cross-model review raised `PidTagWasteBasketEntryId` (0x35E3,
Deleted Items) and `PidTagFinderEntryId` (0x35E7, Search/Finder), and were
declined each time on the reasoning that creating the Deleted Items/Search
folder objects these EntryIDs would need to reference was folder-**tree**
creation work assigned to track **0069**, and that writing an EntryID for a
folder that does not exist in the file would be actively dishonest structure.

**Round 9 reversed this decision** on newly-verified authoritative evidence:
the orchestrator fetched and read the actual MS-PST specification pages at
learn.microsoft.com directly (not from memory) and confirmed two things the
prior rounds had gotten wrong:

1. `PidTagIpmWastebasketEntryId`/`PidTagFinderEntryId` are not "richness"
   properties — they are two of the five properties Microsoft's own page
   documents as the "Minimum Set of Required Properties" for a valid message
   store PC (alongside `PidTagRecordKey`/`PidTagDisplayName`/
   `PidTagIpmSubtreeEntryId`, all three already implemented). See
   https://learn.microsoft.com/en-us/openspecs/office_file_formats/ms-pst/5493a0eb-0356-4e88-b4f5-0433ce0a93fa.
2. The "Top of Personal Folders" (IPM_SUBTREE) required-initialization page
   explicitly documents its hierarchy TC as holding a "Deleted Items" row —
   this track's LOCKED v1 shape was missing that folder and that row, and its
   own IPM_SUBTREE `PidTagDisplayName` was a literal-string bug (writing
   `"IPM_SUBTREE"` instead of the MS-PST-required `"Top of Personal
   Folders"`). See
   https://learn.microsoft.com/en-us/openspecs/office_file_formats/ms-pst/ea4d8b8a-6062-4930-94ee-555527a274d1.

Given that, "creating new folder objects is 0069 scope" no longer held —
these are v1 structural-correctness requirements for the LOCKED store shape
this track already owns, not new-feature richness. What is now implemented:

- **Deleted Items** (`crates/pst-writer/src/production.rs::write_unicode_pst`):
  a real folder object — PC (`PidTagDisplayName = "Deleted Items"`,
  `PidTagContentCount = 0`) + empty hierarchy/contents/associated-contents
  TCs — child of IPM_SUBTREE, referenced as the second row of IPM_SUBTREE's
  hierarchy TC (alongside the existing "Unique Mail" row) and by the store's
  `PidTagIpmWastebasketEntryId`. Always empty — v1 never invents
  deleted-items content, consistent with the "no invented content" principle
  used everywhere else in this track.
- **Search Root** (same file): a real folder object using
  `NID_TYPE_SEARCH_FOLDER` (0x03, verified from
  https://learn.microsoft.com/en-us/openspecs/office_file_formats/ms-pst/2dfb3012-b81c-466b-831c-2d2f0c29e591,
  "the search Folder object is implemented as a PC that is identified by a
  special NID_TYPE of NID_TYPE_SEARCH_FOLDER (0x03)"). Given the same
  PC + hierarchy/contents/associated-contents TC shape as the other folders
  in this file (the safer, more-complete interpretation of "the basic schema
  requirements... are identical to the Folder object PC" over a bare
  PC-only guess). **Not** a child of IPM_SUBTREE's hierarchy TC — the
  verified "Top of Personal Folders" hierarchy-TC row list names only
  Deleted Items — referenced solely via the store's `PidTagFinderEntryId`.
  v1 never implements search-criteria semantics or search-execution logic
  and never populates it with results; it is always empty.
- **Message store PC**: now also carries `PidTagIpmWastebasketEntryId`
  (embedding Deleted Items' NID) and `PidTagFinderEntryId` (embedding Search
  Root's NID), built with the same generalized `build_folder_entry_id`
  helper (renamed from `build_ipm_subtree_entry_id`, now with three call
  sites) and the same store `PidTagRecordKey`-derived `ProviderUID` as
  `PidTagIpmSubtreeEntryId`, for self-consistency.

Same residual as before: these EntryID/NID shapes remain unverified against a
real Outlook-opened PST in this environment (no scanpst/Outlook available —
same constraint as D-0068-02); this document does not assert the store now
opens cleanly in Outlook, only that the previously-missing required
properties/folders are now present per the verified MS-PST specification
text. See
`crates/pst-writer/tests/writer_v1.rs::store_has_wastebasket_and_finder_entry_ids_matching_real_folder_nids`,
`ipm_subtree_hierarchy_resolves_unique_mail_and_deleted_items_by_name`, and
`ipm_subtree_has_required_top_of_personal_folders_initialization`.

### MS-PST "template objects" (NID range 0x60D–0x610) — implemented (round 9; supersedes the round-6 decline)

The round-6 review also asked for MS-PST "template objects" — fixed-NID,
always-zero-row Hierarchy/Contents/AssocContents/SearchContents Table
Template objects — and this was declined on the reasoning that they are an
**Outlook-internal creation-time optimization** (consulted only when
Outlook's own UI clones one to interactively create a *new* folder), not
something a reader needs to open and traverse an *existing* file's real
per-folder tables.

**Round 9 re-verified this directly against the four individual MS-PST
specification pages** (not from memory) rather than relying on the round-6
general characterization, and found each page states its table template
"MUST have no data rows" as a structural requirement of a valid PST — i.e.
these are real fixed top-level nodes the file format expects to exist, not
merely an Outlook UI convenience that a reader can ignore. Implemented as
four always-empty TCs at their fixed, verified NIDs:

| Template | NID | Columns | Source |
|---|---|---|---|
| Hierarchy Table Template | `0x60D` | 13 | https://learn.microsoft.com/en-us/openspecs/office_file_formats/ms-pst/c08fb6cb-2d91-42e5-b70d-f3e4f9781a2a |
| Contents Table Template | `0x60E` | 27 | https://learn.microsoft.com/en-us/openspecs/office_file_formats/ms-pst/f58e1ea9-b592-408d-b89e-53fd4cd6024b |
| FAI Contents Table Template | `0x60F` | 14 | https://learn.microsoft.com/en-us/openspecs/office_file_formats/ms-pst/b2e619a0-6a9c-4101-9dcb-340ac41cf308 |
| Search Folder Contents Table Template | `0x610` | 18 | https://learn.microsoft.com/en-us/openspecs/office_file_formats/ms-pst/cdcf9571-049f-47f5-b075-8374057134ec |

Each is registered as its own top-level node (`Layout::add_node_data(NID_*,
heap_bytes, 0, 0)`, no parent/subnode — the same pattern already used for
`NID_MESSAGE_STORE`/`NID_NAME_TO_ID_MAP`), built via a new
`build_template_tc_columns` helper (`crates/pst-writer/src/production.rs`)
that groups each table's real column schema widest-first (8-byte, then
4-byte, then 1-byte, per MS-PST §2.3.4.1's TCINFO row-layout convention),
computes correct running `ib_data` byte offsets, and appends the
existence-bitmap tail — every column gets a real TCOLDESC and a correct row
width even though the table itself always has zero data rows, since a reader
still needs to parse the TCINFO column schema without error.

**Judgment call, explicitly flagged:** the FAI Contents Table Template's
`0x6805` column is `PtypMultipleInteger32` (a MAPI multi-value type this
repo's TC column model has no prior precedent for). Per the source data's own
guidance, it is modeled conservatively as a 4-byte HNID reference — identical
in *width* to the existing `PtypString`/`PtypBinary` HNID-reference
convention — never as an inline fixed-size value. This is never exercised
beyond column-width bookkeeping (the table has zero rows in v1 regardless),
so no real multi-value storage/decoding was implemented or tested.

Also verified: the Search Folder Contents Table Template's own published
source page lists `0x0E07`/`0x0E17` twice among its columns — treated as a
documentation quirk on Microsoft's page (a TC cannot have a duplicate column
tag) and included once each here, per the explicit instruction accompanying
the verified data.

Same residual as before: not independently verified against a real
Outlook-opened PST or `scanpst.exe` in this environment (same constraint as
D-0068-02). See
`crates/pst-writer/tests/writer_v1.rs::fixed_template_object_tables_are_present_and_empty`.

## `PidTagMessageSize` formula (§3.3.2)

Computed per message from bytes **actually written**, never copied from a
source/declared size:

```text
message_size = len(PC heap bytes, computed WITHOUT the MessageSize property
                    itself — it is self-referential)
             + len(UTF-16LE bytes of body_plain), if diverted to a subnode
             + len(bytes of body_html), if diverted to a subnode
             + per written attach: attach PC len (+ raw bytes if subnode-diverted;
               or + nested size for embeds)
             + len(finalized attachment-table heap), when attaches present
               (real table size — never a fabricated constant)
```

The "without MessageSize itself" step avoids circularity: the PC is built once
to measure its size, then rebuilt with `PidTagMessageSize` appended using that
measurement. This under-counts by the ~8 bytes `PidTagMessageSize`'s own BTH
leaf record contributes (a fixed, negligible, documented constant) — it is
never inflated by a source-declared value, which is the property this exists
to guarantee (see `body/message_size_is_computed_not_copied_from_inflated_source`
test: a `CanonicalMessage` with a fake 50,000,000-byte declared size and a tiny
actual body yields a small stored `PidTagMessageSize`).

## Soft-body fidelity flags (`body_incomplete` / `body_unavailable`) (§2.4)

`WriteMessage.body_incomplete` and `WriteMessage.body_unavailable` are
reporting-only flags — neither is ever written as a MAPI property. Per spec
§2.4 ("Deferred roll-in"): if a message's body is incomplete or unavailable,
it is still written with whatever other properties are available (subject,
sender, message-id, etc.) plus an empty/partial body — the writer never
invents body content to fill the gap (`body_unavailable = true` forces `None`
for both plain and HTML regardless of what `body_plain`/`body_html` contain;
see the fidelity matrix above).

So a caller has visibility into this from the write report alone (not just by
re-inspecting every input `WriteMessage`), `WritePstReport` carries two
additive counters populated during the write loop in `write_unicode_pst`:

- `messages_with_incomplete_body: u64` — count of written messages where
  `body_incomplete` was `true`.
- `messages_with_unavailable_body: u64` — count of written messages where
  `body_unavailable` was `true`.

A message with both flags set counts toward both counters independently; they
are not mutually exclusive. These are purely additive to the existing report
shape — `messages_written`/`messages_skipped`/`bytes`/`path` are unchanged.
See `crates/pst-writer/tests/writer_v1.rs::report_counts_incomplete_and_unavailable_bodies`.

## Large single-property values: subnode storage

This writer's `HeapBuilder` is **single-page** (`MAX_BLOCK_DATA` = 8176). Values
that would overflow that page are moved to a **subnode** (NID in `dwValueHnid`)
instead of being clipped. MS-PST itself allows multi-block HN (§2.3.1.6) and a
format per-value threshold of **3580** before subnode (§2.6.1.2.2 / §2.6.2.3.2);
our inlined threshold **`MAX_HEAP_VALUE_SIZE` = 2048** is a **documented
single-page HeapBuilder deviation**, not an inherent MS-PST limit (track **0093**).

Any `body_plain` / `body_html` above 2048 bytes (post-encoding) is diverted to a
subnode. Helper strings (MID / subject / sender / Display* / `message_class`)
use the same per-value helper **and** a **cumulative** escalate+reprobe on the
MessageSize probe heap so multiple ~2 KiB helpers cannot hard-fail the page.
`pst-reader`'s `PropContext` resolves subnode-typed HNIDs for `PtypString` /
`PtypBinary` so round-trip verification works.

Bodies larger than one external data block (8176 bytes) always use
XBLOCK/XXBLOCK chaining regardless of whether they were inline or
subnode-diverted — there is no size at which this writer silently truncates.

`Layout::write_data_chain` checks size in two stages, in this order:

1. **Practical maximum: `i32::MAX` bytes (~2 GiB).** Any single value
   (`body_plain`/`body_html`) larger than `i32::MAX` bytes is rejected
   immediately, before any XBLOCK/XXBLOCK planning happens. This ceiling is
   **not** an XBLOCK/XXBLOCK structural limit — `lcbTotal` in those headers is
   a 4-byte *unsigned* field and could describe values up to `u32::MAX` (~4
   GiB) just fine. The tighter bound comes from `PidTagMessageSize` (MAPI tag
   `0x0E08`), which every written message carries: it is a `PtypInteger32` /
   `PT_LONG` property per MS-OXPROPS — a 32-bit **signed** integer whose
   representable range is `0..=i32::MAX` (~2 GiB). Since every message's
   `PidTagMessageSize` must be able to honestly report the size of any body it
   contains, no single value the writer accepts may itself exceed what that
   property can represent — even though the XBLOCK/XXBLOCK chain mechanics
   underneath could physically store more. This hard-fails with
   `WriterError::BodyTooLarge`, not `AllocationFailed`.
2. **Structural XBLOCK/XXBLOCK entry-count capacity (theoretical, larger than
   #1 and so never actually reached in v1):** one XBLOCK holds up to 1021
   external blocks (~8.35MB); an XXBLOCK of XBLOCKs raises that to ~8.5GB.
   This ceiling exists in the code (`write_data_chain` errors with
   `WriterError::AllocationFailed` if planning ever produced more XBLOCKs
   than one XXBLOCK can reference) but is unreachable in practice because the
   `i32::MAX` check above always rejects the input first — ~8.5GB is larger
   than ~2 GiB.

Net effect: the **practical maximum representable single-value size in v1 is
bounded by `i32::MAX` (~2 GiB), tied to `PidTagMessageSize`'s PT_LONG range**,
not to XBLOCK/XXBLOCK's own (larger, and now practically irrelevant)
structural capacity — and the error an oversize value actually gets back is
`WriterError::BodyTooLarge`, not `AllocationFailed`. `AllocationFailed`
remains reachable code for the XBLOCK/XXBLOCK entry-count ceiling itself, but
only as defensive/documentation value — it does not describe v1's real-world
limit.

As defense-in-depth, the computed `PidTagMessageSize` value itself (PC heap
bytes + any subnode-diverted body/html bytes + structural overhead) is also
converted with a hard, non-silent `i32::try_from` — never clamped — so that
even a hypothetical future path that could push the *total* past `i32::MAX`
(e.g. some other change growing per-message overhead) fails loudly with
`WriterError::BodyTooLarge` instead of silently misreporting a smaller size
than what was actually written. In v1, stage 1 above always rejects an
oversized `body_plain`/`body_html` first, so this second check is expected to
be unreachable in practice.

## Output safety (§3.7)

`write_unicode_pst`'s signature is:

```rust
pub fn write_unicode_pst(
    path: &Path,
    messages: impl IntoIterator<Item = WriteMessage>,
    protected_source_paths: &[PathBuf],
    opts: &WritePstOpts,
) -> Result<WritePstReport>
```

Two independent safety checks, run in this order:

1. **Hard, non-overridable refusal — protected source inputs (§3.7 rule 2).**
   `protected_source_paths: &[PathBuf]` is a **mandatory function parameter**
   of `write_unicode_pst`, not a field on `WritePstOpts`. It used to be a
   `WritePstOpts` field defaulting to `Vec::new()`, which meant a completely
   ordinary call like `WritePstOpts::default()` or `WritePstOpts { overwrite:
   true, ..Default::default() }` got zero source-overwrite protection with no
   compiler warning, no runtime warning, and no other friction — the
   protection only existed if the caller happened to remember to populate
   that one specific struct field. Promoting it to a required, separate
   function parameter forces every call site to type *something* for it, even
   a deliberately empty `&[]` — an empty slice is now a conscious, visible
   choice to opt out, not an invisible default. This crate deliberately does
   not parse or track source PSTs itself (that's the caller's — e.g. a future
   0069/0071 CLI's — responsibility), so this still cannot force the caller to
   supply the *correct* or *complete* set of paths; that residual trust
   boundary is inherent to any library that doesn't independently know its
   caller's inputs. `write_unicode_pst` refuses — typed
   `WriterError::RefusedSourceOverwrite`, checked **before and independently
   of** the generic overwrite check below — if the destination `path` matches
   (by best-effort canonicalized comparison; falls back to the literal path
   when the destination doesn't exist yet, since `canonicalize()` requires an
   existing file) any entry in `protected_source_paths`. **`WritePstOpts::overwrite
   = true` never bypasses this.** This is the concrete enforcement of Core
   Mandate #3 ("This project is read-only against PST inputs. Do not mutate
   PST files.") and of spec §3.7 rule 2 ("Refuse to write onto any input PST
   path" — always, no override). A caller that passes `&[]` gets no
   protection from this check beyond the generic overwrite-refusal below —
   callers that know their input PST paths (0069/0071) are expected to pass
   them in.

   **This check covers both the final destination and the computed
   temp-staging path (review round 8 P2 fix).** `write_unicode_pst` writes
   the entire file to a computed temp sibling of `path` (see below) via
   `File::create`, *before* the safety-relevant `fs::rename` step, and only
   the rename target used to be compared against `protected_source_paths`.
   That left a real gap: a protected source PST whose path happened to equal
   the computed temp-sibling name would have been silently truncated by
   `File::create` during staging — the rename-target check would never even
   run, because the file had already been overwritten before that point.
   `write_unicode_pst` now runs the identical protected-source check (a
   shared `check_not_protected_source` helper, not a hand-duplicated
   variant) against the temp path too, immediately after computing it and
   strictly before `File::create` is ever called on it — so both paths this
   writer will actually touch are guarded, not just the one it touches last.
2. **Default refusal, legitimately overridable — stale output (§3.7 rule 3).**
   Refuses (typed `WriterError::Refused`) to write when the destination
   already exists, unless `WritePstOpts::overwrite = true`. Unlike the
   protected-source check, this one is *by design* overridable: it exists to
   stop accidental clobbering of stale output, not to protect an input.

Both checks happen before any file is created. Writes go to a
`<filename>.tmp-<pid>-<entropy>` sibling of the destination (`pub fn
temp_sibling_path`, exported so the integration test suite can call the same
function `write_unicode_pst` uses internally rather than re-guessing its
naming scheme), then `fs::rename`s over the destination only after the full
file is written successfully (Windows `rename` replaces an existing
destination file) — `write_unicode_pst` never mutates an existing file in
place either way.

**Temp-name entropy (review round 8 P2 fix).** The temp-sibling name used to
be purely `<filename>.tmp-<pid>` — deterministic from the destination
filename and the process ID alone. Current (2026) Rust guidance on safe
atomic file writes treats deterministic temp names as a known collision
hazard (this is exactly why crates like `tempfile` exist, layered under
higher-level helpers like `atomicwrites`): PIDs are reused across process
lifetimes and form a small, predictable space, so a stale temp file left by
a previous crashed run — or, worst case, an adversarial or mistaken input
file that happens to already carry that exact name — could collide with it.
This crate does not add a new dependency (`tempfile`/`uuid`/`rand`) for
this; instead it uses dependency-free process entropy via
`process_entropy_suffix` (wall-clock nanoseconds + PID, 8-hex `crc32fast`
cache per process — **staging only**, not final PST store identity; store
RecordKey determinism is track **0087** / `derive_store_record_key`).
Repeated calls for the same destination within one run — including the
integration test calling `temp_sibling_path` directly to predict the exact
value `write_unicode_pst` will compute — agree, while a different process
(a later run, a restarted one, or an attacker without this process's
PID/start time) gets a different suffix.
This is explicitly *defense in depth*, not the sole guarantee: it reduces
the ambient chance of a collision, while the explicit
`protected_source_paths` check above (which now also covers the temp path)
is what actually guarantees a collision is refused rather than silently
written through.

## No silent truncation / no unwrap in the production path

- `write_unicode_pst` and everything it calls returns `Result` and never
  reaches the fixture path's `assert!`-based `Layout::add_node`. It grows node
  data via XBLOCK/XXBLOCK (`Layout::write_data_chain`) instead.
- Values that would overflow one heap page hard-fail (`WriterError::Layout`)
  rather than silently corrupt/truncate a page, *unless* the writer proactively
  diverts them to a subnode first (body/HTML **and** helper strings under the
  0093 cumulative budget — never silent Display* clip).

## CanonicalMessage → WriteMessage adapter

`pst-writer` takes a normal crate dependency on `dedup-engine` for exactly one
function, `pst_writer::from_canonical_message(&CanonicalMessage) -> (WriteMessage, u64)`.
No cycle is introduced (`dedup-engine` does not depend on `pst-writer`).
Attachments are **mapped** (0069) along with `locus.folder_path` /
`locus.source_path`. The `u64` is the count of attachments the adapter could
not represent (0 today — reserved for a future 0071 CLI aggregate).

## wSig (page signature)

`pst-reader` does not validate `wSig` at all, but real Outlook/`scanpst` do.
v1 computes it as `(ib ^ bid_lo ^ bid_hi)` folded to 16 bits (low/high 32-bit
halves XORed together) — a widely cross-referenced approach for this field.
This has **not** been independently verified against a real Outlook-opened PST
in this environment (scanpst/Outlook unavailable here) — flagged as a residual
alongside the EntryID note above.

## CLI

No `pst-dedup write-pst` subcommand was added in this track. Per spec §3.11
this is preferred-but-not-required; the hard DoD gate is the library API plus
tests. Given the amount of correctness work required to get XBLOCK/XXBLOCK,
subnode storage, real sorted BTree keys, and the hierarchy/EntryID right, a CLI
subcommand was left to track **0071** (which already owns the end-to-end
keep-set → write-pst → report wiring) rather than risk trading off writer
correctness for CLI surface.
