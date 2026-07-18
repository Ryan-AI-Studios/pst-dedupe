# PST-Dedup: Architecture & Implementation Blueprint

## Project Overview

**Purpose:** Standalone Windows tools (Rust) that deduplicate emails across multiple PST
files. Surfaces: **CLI** (`pst-dedup`) for scripts/agents and **egui GUI** for interactive
use. Produces a dedup report (CSV) and optionally exports unique emails as EML files.
Target scale: 1M+ emails across multi-gigabyte PSTs.

**Key Constraints:**
- Pure Rust, zero C dependencies — no libpff, no Outlook
- PST reading implemented from [MS-PST] specification v20240820
- Statically-linked Windows `.exe` deployment (CLI and/or GUI)
- Must handle PST files >10GB and aggregate >1M messages
- Commercial/government use — all dependencies must be permissively licensed

---

## Crate Architecture

```
pst-dedup/                      (Cargo workspace)
├── crates/
│   ├── pst-reader/             Pure Rust PST parser (the hard part)
│   │   src/
│   │   ├── lib.rs              Public API: PstFile, Message, Folder iterators
│   │   ├── header.rs           PST header & trailer parsing
│   │   ├── crypto.rs           NDB_CRYPT_PERMUTE & NDB_CRYPT_CYCLIC decoding
│   │   ├── ndb/                Node Database Layer
│   │   │   ├── mod.rs
│   │   │   ├── page.rs         Page types: AMap, PMap, FMap, FPMap, BTreePage
│   │   │   ├── btree.rs        NBT (Node BTree) and BBT (Block BTree) traversal
│   │   │   ├── block.rs        Data blocks, XBLOCK, XXBLOCK (multi-block data)
│   │   │   └── nid.rs          Node ID types and special NID constants
│   │   ├── ltp/                Lists, Tables & Properties Layer
│   │   │   ├── mod.rs
│   │   │   ├── hn.rs           Heap-on-Node allocator
│   │   │   ├── bth.rs          BTree-on-Heap
│   │   │   ├── pc.rs           Property Context (key-value property bags)
│   │   │   └── tc.rs           Table Context (tabular row data)
│   │   └── messaging/          Messaging Layer
│   │       ├── mod.rs
│   │       ├── store.rs        Message Store root + named property map
│   │       ├── folder.rs       Folder hierarchy traversal
│   │       ├── message.rs      Message property extraction
│   │       └── attachment.rs   Attachment metadata (name + size for hashing)
│   │
│   ├── dedup-engine/           Dedup logic (independent of PST format)
│   │   src/
│   │   ├── lib.rs              Public API
│   │   ├── hasher.rs           Tiered hashing: Message-ID → content hash
│   │   ├── index.rs            In-memory dedup index (HashMap-based)
│   │   ├── report.rs           CSV report generation
│   │   └── exporter.rs         EML export (RFC 5322 serialization)
│   │
│   ├── pst-dedup-cli/          Agent/human CLI (`pst-dedup` binary)
│   │   src/
│   │   ├── main.rs             clap: inspect / scan / dups
│   │   ├── scan.rs             Scan orchestration + CSV/JSON summary
│   │   ├── inspect.rs          Folder tree / counts
│   │   └── error.rs            Typed CLI errors
│   │
│   ├── dedupe-desk/            Dedupe Desk product shell (0020)
│   │   src/
│   │   ├── main.rs             eframe entry; window title "Dedupe Desk"
│   │   ├── app.rs              DeskApp: ProcessRunner, nav, exit shutdown
│   │   ├── matter_ui.rs        create/open + open_for_read snapshot (WAL)
│   │   ├── workspace.rs        sources / PSTs / jobs / process actions
│   │   ├── progress_ui.rs      watch progress + request_repaint_after(100ms)
│   │   ├── dialogs.rs          off-thread rfd + dialog_open debounce
│   │   ├── nav.rs / params.rs  pure helpers (unit tested)
│   │   └── settings.rs         recent matter paths (JSON under APPDATA)
│   │
│   ├── pst-dedup-gui/          Legacy scan/dedup wizard (regression)
│   │   src/
│   │   ├── main.rs             Entry point, eframe setup
│   │   ├── app.rs              Top-level App struct, state machine
│   │   ├── views/
│   │   │   ├── file_select.rs  PST file picker panel
│   │   │   ├── settings.rs     Dedup config panel
│   │   │   ├── progress.rs     Scan progress with throughput stats
│   │   │   └── results.rs      Report viewer + export controls
│   │   └── worker.rs           Background thread for PST processing
│   │
│   ├── pst-writer/             Experimental write path / fixture helpers
│   │
│   ├── matter-core/            Matter store foundation (Desk tracks 0015+)
│   │   src/
│   │   ├── lib.rs              Public API: Matter, CAS, audit, jobs, items, logical_hash
│   │   ├── matter.rs           Layout create/open + items/family high-level API
│   │   ├── schema.rs           Versioned SQLite migrations (schema v2)
│   │   ├── logical_hash.rs     Desk logical_hash v1 (length-prefixed; BCC-aware)
│   │   ├── cas.rs              SHA-256 CAS (put_bytes + streaming put_reader)
│   │   ├── audit.rs            Append-only audit log + hash chain verify
│   │   ├── jobs.rs             Jobs + checkpoint resume primitives
│   │   ├── item_errors.rs      Item-level error accumulator
│   │   └── error.rs            Typed thiserror errors
│   │
│   ├── ingest-purview/         Purview/package/ZIP detect + safe expand (0016)
│   │   src/
│   │   ├── lib.rs              Public API: detect, ingest_path, ingest_path_on_job, resume_ingest
│   │   ├── detect.rs           Package kind heuristics
│   │   ├── path_safety.rs      Path sanitize + property tests
│   │   ├── encoding.rs         ZIP name UTF-8/CP437/Win-1252 fallbacks
│   │   ├── expand.rs           Nested ZIP expand, leaf checkpoints, CAS
│   │   ├── ingest.rs           Matter source/job/audit wiring (Option C on_job)
│   │   ├── limits.rs           ExpandLimits defaults
│   │   └── error.rs            Typed thiserror errors
│   │
│   ├── extract-pst/            PST → Normalized Items (0018; blocking)
│   │   src/
│   │   ├── lib.rs              Public API: extract_pst_* , *_on_job, resume_extract
│   │   ├── open.rs             FS vs CAS → workspace/temp (never %TEMP%)
│   │   ├── extract.rs          Walk + batch checkpoints + families
│   │   ├── native_message.rs   pst-native-message-v1 (not EML)
│   │   ├── recipients.rs       Display* parse; BCC never invented
│   │   ├── checkpoint.rs       Mid-folder cursor
│   │   ├── limits.rs           ExtractLimits / ExtractSummary
│   │   └── error.rs            Structured extract codes
│   │
│   └── process-runner/         In-process job runner (0019)
│       src/
│       ├── lib.rs              ProcessRunner, CancelToken, watch progress
│       ├── runner.rs           Single matter worker + Drop join
│       ├── progress.rs         tokio::sync::watch (+ optional broadcast)
│       ├── handler.rs          JobHandler trait / JobContext
│       └── handlers/           IngestHandler, ExtractPstHandler (features)
```

### Dedupe Desk shell (0020)

Primary product binary: **`dedupe-desk.exe`**. UI thread may only call
`ProcessRunner::start` / `resume` / `cancel` / `watch_progress` / `shutdown`.
List refresh uses `Matter::open_for_read` (WAL concurrent with the worker).
Native dialogs run off-thread with a single `dialog_open` gate. While a job
is Running, repaint is throttled with `request_repaint_after(100ms)` — never
free-run `request_repaint()`. See `crates/dedupe-desk/README.md`.

### Matter on-disk layout (`matter-core`)

Caller-chosen root (e.g. `Matters/<id>/`):

```text
matter.db                 # SQLite metadata (WAL)
blobs/sha256/<aa>/<hex>   # CAS: raw physical bytes only (streaming put_reader)
index/                    # reserved (Tantivy FTS)
exports/                  # reserved (production sets)
logs/                     # optional file logs
workspace/temp/           # extractor spill; cleaned on Matter open/create
```

See `crates/matter-core/README.md` for CAS, audit, Normalized Item (schema v2),
family graph, and logical_hash v1 contracts. See `crates/extract-pst/README.md`
for PST extract (blocking thread, native v1, mid-folder resume). See
`crates/process-runner/README.md` for the single matter-worker runner, watch
progress, Option C job-id injection, and cancel/Drop join. See
`crates/dedupe-desk/README.md` for the product shell UI contracts.

---

## MS-PST Format: Layer-by-Layer Implementation Guide

### Reference Spec
[MS-PST]: Personal Folder File (.pst) Format — Microsoft Open Specifications
URL: https://learn.microsoft.com/en-us/openspecs/office_file_formats/ms-pst/

**CRITICAL:** Only implement **Unicode PST** (versions 23/36, wVer=23 dwMagic=!BDN).
ANSI PST (wVer=14-15) is legacy and rare in government contexts. Detect and reject
with a clear error message.

---

### Layer 0: Header (§2.2.2)

```
Offset  Size  Field
------  ----  -----
0       4     dwMagic = 0x4E444221 ("!BDN" read as little-endian u32)
4       4     dwCRCPartial
8       2     wMagicClient = 0x4D53 ("SM")
10      2     wVer (23 = Unicode, 14/15 = ANSI → reject)
12      2     wVerClient
14      1     bPlatformCreate
15      1     bPlatformAccess
16      4     dwReserved1
20      4     dwReserved2
24      8     bidUnused
32      8     bidNextP
36      4     dwUnique
40     128    rgnid[32] — NID counters per type
168     8     qwUnused
180    72     root — ROOT structure (see below)  [Unicode offset 0xB4]
252    4      dwAlign
256    128    rgbFM — initial FMap (deprecated)  [Unicode 0x100]
384    128    rgbFP — initial FPMap (deprecated) [Unicode 0x180]
512    1      bSentinel = 0x80                  [Unicode 0x200]
513    1      bCryptMethod — 0=None, 1=Permute, 2=Cyclic  [Unicode 0x201]
514    2      rgbReserved
516    8      bidNextB
524    4      dwCRCFull
…             reserved / pad toward 4K page
```

> **Implementation note (2026-07):** Older drafts mis-aligned Unicode `rgbFM`/`rgbFP`
> (508-byte skip) and over-read ROOT padding (7 bytes after `fAMapValid`). That shifted
> `bCryptMethod` and caused encrypted PSTs to be treated as unencrypted. Correct layout
> is above; code lives in `pst-reader` `header.rs`.

**ROOT Structure** (§2.2.2.5 / §2.2.2.6, at header offset 180 / `0xB4`, 72 bytes for Unicode):
```
Offset  Size  Field
------  ----  -----
0       4     dwReserved
4       8     ibFileEof — file size in bytes
8       8     ibAMapLast — offset of last AMap page
16      8     cbAMapFree — total free space in AMaps
24      8     cbPMapFree — (deprecated)
32      16    BREFNBT — BREF to root page of Node BTree
48      16    BREFBBT — BREF to root page of Block BTree
64      1     fAMapValid — 1 if AMap is valid
65      1     bReserved
66      2     wReserved
```

**BREF** (§2.2.2.4, 16 bytes for Unicode):
```
Offset  Size  Field
------  ----  -----
0       8     bid — Block ID
8       8     ib  — byte offset in file (absolute)
```

**Implementation Notes:**
- Parse header, validate magic bytes, reject ANSI
- Extract bCryptMethod — needed for all block reads
- Extract BREFNBT.ib and BREFBBT.ib — entry points to the two B-trees
- All multi-byte integers are little-endian

---

### Layer 1: NDB — Node Database (§2.2.2)

The NDB is a block-level storage engine. Everything above it is built on nodes.

#### Block IDs (BIDs, §2.2.2.2)
- 8 bytes for Unicode
- Bit 0: `fInternal` — 0 = data block, 1 = internal (XBLOCK/XXBLOCK/SLBLOCK/SIBLOCK)
- Bits 1-63: block counter value
- BID=0 means "no block" / null reference

#### Pages (§2.2.2.7) — Always 512 bytes

Every page ends with a **page trailer** (§2.2.2.7.1, last 16 bytes for Unicode):
```
Offset  Size  Field
------  ----  -----
0       1     ptype — page type
1       1     ptypeRepeat — must equal ptype
2       2     wSig — signature (computed from ib and bid)
4       4     dwCRC — CRC32 of page data (bytes 0..496)
8       8     bid — BID of this page
```

Page types (ptype):
- `0x80` = BBT page
- `0x81` = NBT page
- `0x82` = FMap (deprecated, ignore)
- `0x83` = PMap (deprecated, ignore)
- `0x84` = AMap (allocation map)
- `0x85` = FPMap (deprecated, ignore)
- `0x86` = DList (density list)

#### BTree Pages (§2.2.2.7.7)

Both NBT and BBT are stored as B-trees of 512-byte pages.

**BTPAGE layout:**
```
Offset  Size    Field
------  ------  -----
0       488     rgentries — array of BTree entries
488     1       cEntries — count of entries
489     1       cEntMax — max entries for this page
490     1       cbEntKey — key size in bytes
491     1       cLevel — 0 = leaf, >0 = intermediate
492     4       dwPadding
496     16      pageTrailer
```

**Intermediate entries (cLevel > 0):**
For NBT (cbEnt=24 Unicode): `key(8) + BREF(16)` — key is NID, BREF points to child page
For BBT (cbEnt=24 Unicode): `key(8) + BREF(16)` — key is BID, BREF points to child page

**Leaf entries differ by tree type:**

NBT Leaf Entry (NBTENTRY, §2.2.2.7.7.4, 32 bytes Unicode):
```
nid         8 bytes — Node ID (the key)
bidData     8 bytes — BID of data block/tree
bidSub      8 bytes — BID of subnode BTree (0 if none)
nidParent   4 bytes — parent NID
dwPadding   4 bytes
```

BBT Leaf Entry (BBTENTRY, §2.2.2.7.7.3, 24 bytes Unicode):
```
BREF       16 bytes — bid(8) + ib(8) → block location on disk
cb          2 bytes — size of data in block (raw, before decompression)
cRef        2 bytes — reference count
dwPadding   4 bytes
```

#### Data Blocks (§2.2.2.8)

External (non-internal) blocks contain raw data. Layout:
```
Offset  Size         Field
------  -----------  -----
0       cb           data bytes (the payload)
cb      padding      pad to 64-byte alignment
...     16           BLOCKTRAILER — dwCRC(4) + bid(8) + ...
```

Block max payload: **8176 bytes** (8192 − 16 trailer) for Unicode.
Data is encrypted per bCryptMethod before storage — decrypt after reading.

#### Multi-Block Data: XBLOCK & XXBLOCK (§2.2.2.8.3)

When data > 8176 bytes, it's split across multiple data blocks referenced by an XBLOCK
(or XXBLOCK for very large data).

**XBLOCK** (btype=0x01, cLevel=0x01):
```
btype       1 byte  = 0x01
cLevel      1 byte  = 0x01
cEntries    2 bytes — count of BIDs
lcbTotal    4 bytes — total uncompressed size
rgBIDs      8*cEntries bytes — array of data block BIDs (in order)
padding + BLOCKTRAILER
```

**XXBLOCK** (btype=0x01, cLevel=0x02):
Same structure but each BID points to an XBLOCK, not a data block.

#### Subnode BTree (§2.2.2.8.3.3)

Some nodes contain sub-nodes (used by LTP for row data). The bidSub in an NBT entry
points to an SLBLOCK or SIBLOCK.

**SLBLOCK** (leaf, btype=0x02, cLevel=0x00):
```
btype       1 byte  = 0x02
cLevel      1 byte  = 0x00
cEntries    2 bytes
dwPadding   4 bytes
rgentries   cEntries × SLENTRY
```

SLENTRY (24 bytes Unicode): `nid(8) + bidData(8) + bidSub(8)`

**SIBLOCK** (intermediate, btype=0x02, cLevel=0x01):
Same but entries are `nid(8) + bid(8)` pointing to child SLBLOCKs.

#### Encryption (§5.1)

**NDB_CRYPT_PERMUTE** (bCryptMethod=1): byte-level substitution cipher.
Apply mpbbCrypt table (§5.1, 256-byte lookup) to each byte of data block content.
Decode: `decoded[i] = mpbbR[encoded[i]]` where mpbbR is the reverse table.

**NDB_CRYPT_CYCLIC** (bCryptMethod=2): XOR-based cipher using a key derived from BID.
```
key = bid ^ (bid >> 16)  // 32-bit key
For each byte at offset i:
    decoded[i] = encoded[i] ^ key_byte[(i % 4)]
```
where key_byte is the 4 bytes of the 32-bit key.

**Only data block payloads are encrypted.** Page data, block trailers, and internal
blocks (XBLOCK/XXBLOCK/SLBLOCK/SIBLOCK) are NOT encrypted.

---

### Layer 2: LTP — Lists, Tables & Properties (§2.3)

Built on top of NDB nodes. This layer interprets node data as structured property storage.

#### Heap-on-Node (HN, §2.3.1)

Treats a node's data as a heap with fixed-size allocation pages.

**HNHDR** (at start of first data block of the node):
```
ibHnpm      2 bytes — offset to HN page map
bSig        1 byte  = 0xEC
bClientSig  1 byte  — indicates what's stored: 0xBC=TC, 0x7C=BTH, 0x6C=PC(via BTH)
hidUserRoot 4 bytes — HID of the client's root structure
rgbFillLevel 4 bytes
```

**HID** (Heap ID, 4 bytes): `hidIndex(11 bits) | hidBlockIndex(16 bits) | hidType(5 bits)`
- hidType must be 0
- hidBlockIndex: 0-based index of data block within the node
- hidIndex: 1-based index into the HN page map of that block

**HN Page Map / HNPAGEMAP** (at offset ibHnpm within each data block, §2.3.1.5):
```
cAlloc      2 bytes — number of allocations
cFree       2 bytes — free entries (must be skipped when indexing)
rgibAlloc   (cAlloc+1) × 2 bytes — offsets within the block
```
Item `i` occupies bytes `rgibAlloc[i-1]` to `rgibAlloc[i]` (1-indexed; rgibAlloc[0]
is the start of allocatable space). Omitting `cFree` misaligns all HID resolutions
and truncates TCINFO / BTH structures.

#### BTree-on-Heap (BTH, §2.3.2)

A B-tree stored inside an HN. Root is at hidUserRoot of the HNHDR.

**BTHHEADER** (at the HID):
```
bType       1 byte  = 0xB5
cbKey       1 byte  — key size (2, 4, 8, or 16)
cbEnt       1 byte  — data size per entry
bIdxLevels  1 byte  — 0 = all data in records, >0 = intermediate levels
hidRoot     4 bytes — HID of root, 0 if empty
```

Leaf records: `key(cbKey) + data(cbEnt)` — packed sequentially in the HN allocation.
Intermediate records: `key(cbKey) + hidChild(4)` — hidChild points to next level.

#### Property Context (PC, §2.3.3)

A PC is a BTH where:
- cbKey = 2 (property ID, u16)
- cbEnt = 6: `wPropType(2) + dwValueHnid(4)`

**dwValueHnid interpretation:**
- If the property type is "fixed size" and fits in 4 bytes → value is inline
- If variable-size → dwValueHnid is an HID pointing to the data in the HN
- If the data is too large for the HN → it's a subnode NID (check bidSub)

**Property types (wPropType):**
- 0x0002 = PtypInteger16
- 0x0003 = PtypInteger32
- 0x000B = PtypBoolean
- 0x0014 = PtypInteger64
- 0x001F = PtypString (UTF-16LE, variable length)
- 0x0040 = PtypTime (FILETIME, 8 bytes)
- 0x0048 = PtypGuid (16 bytes)
- 0x0102 = PtypBinary (variable length)
- 0x101F = PtypMultipleString
- 0x1102 = PtypMultipleBinary

#### Table Context (TC, §2.3.4)

A TC is a table (rows × columns) built on an HN + subnode BTree.

**TCINFO** (at hidUserRoot; HN `bClientSig` is typically `0x7C` for TC):
```
bType       1 byte  = 0x7C
cCols       1 byte  — column count
rgib[4]     4×2 bytes — offsets for 4/8/variable-size column groups
hidRowIndex 4 bytes — HID of row index BTH
hnidRows    4 bytes — HID or NID containing row data
rgTCOLDESC  cCols × TCOLDESC
```

**TCOLDESC** (8 bytes per column):
```
wPropId     2 bytes — MAPI property tag
wPropType   2 bytes
ibData      2 bytes — offset within row data
cbData      1 byte  — size of data in row
iBit        1 byte  — bit index for cell existence bitmap
```

Row data is stored inline in the HN or in subnode BTree entries. The **RowIndex BTH**
(`hidRowIndex`) maps `dwRowID` → matrix row index. For folder hierarchy and contents
tables, **dwRowID is the child folder/message NID** — do not rely solely on a
`PidTagLtpRowId` (0x67F2) column; many real PSTs omit that column.

---

### Layer 3: Messaging (§2.4)

#### Special NIDs (§2.4.1)

```rust
const NID_MESSAGE_STORE: u64       = 0x21;   // Message store PC
const NID_NAME_TO_ID_MAP: u64      = 0x61;   // Named property mapping
const NID_ROOT_FOLDER: u64         = 0x122;  // Root folder object
```

Folder-related NIDs follow a pattern:
- Folder object:       `nidType=0x02, nidIndex=folder_counter`
- Hierarchy table:     `nidType=0x0D, nidIndex=folder_counter`  
- Contents table:      `nidType=0x0E, nidIndex=folder_counter`
- Associated contents: `nidType=0x0F, nidIndex=folder_counter`

NID composition: `nid = (nidIndex << 5) | nidType`

#### Folder Traversal

1. Look up NID_ROOT_FOLDER (0x122) in NBT → load folder PC (display name)
2. Hierarchy table NID = `(folder_nid & !0x1F) | 0x0D` (for root: `0x12D`)
3. Contents table NID = `(folder_nid & !0x1F) | 0x0E` (for root: `0x12E`)
4. Load each TC; resolve child NIDs from the **RowIndex BTH** (RowID per row)
5. Recurse hierarchy; for each contents NID, read message properties from the PC
   
   **Correct approach:** For any folder with `nid`:
   - `nidHierarchy = (nid & 0xFFFFFFE0) | 0x0D`
   - `nidContents  = (nid & 0xFFFFFFE0) | 0x0E`
   
3. The hierarchy TC's rows have column PidTagNid (0x67F2) → child folder NIDs
4. The contents TC's rows have column PidTagNid (0x67F2) → message NIDs
5. Recurse into child folders

#### Message Properties We Need

| Property | Tag | Type | Purpose |
|---|---|---|---|
| PidTagInternetMessageId | 0x1035 | String | Tier 1 dedup key |
| PidTagSubject | 0x0037 | String | Tier 2 hash input |
| PidTagClientSubmitTime | 0x0039 | PtypTime | Tier 2 hash input |
| PidTagSenderEmailAddress | 0x0C1F | String | Tier 2 hash input |
| PidTagSenderSmtpAddress | 0x5D01 | String | Fallback sender |
| PidTagBody | 0x1000 | String | Tier 2 hash (first 4KB) |
| PidTagDisplayTo | 0x0E04 | String | Report: recipients |
| PidTagMessageSize | 0x0E08 | Integer32 | Report: size stats |
| PidTagHasAttachments | 0x0E1B | Boolean | Attachment detection |

For attachment metadata (name + size hashing):
- Attachment table is a subnode of the message node
- PidTagAttachFilename (0x3704) / PidTagAttachLongFilename (0x3707)
- PidTagAttachSize (0x0E20)

#### Named Properties (§2.4.7) — PidTagNameToIdMap

Some properties use named IDs in the range 0x8000-0x8FFF. The named property map at
NID 0x61 maps these to MAPI property names/GUIDs. Implement this only if needed
(most dedup-critical properties use well-known tags < 0x8000).

---

## Dedup Engine Design

### Tiered Hashing Strategy

```
Tier 1: Message-ID Match (fast, definitive)
  key = normalize(PidTagInternetMessageId)
  normalize: lowercase, trim whitespace and angle brackets

Tier 2: Content Hash (fallback for missing Message-ID)
  key = SHA-256(
    normalize(PidTagSubject) +
    PidTagClientSubmitTime as epoch_millis +
    normalize(PidTagSenderEmailAddress) +
    first_4096_bytes(PidTagBody) +
    sorted(attachment_name + ":" + attachment_size for each attachment)
  )
```

### Dedup Index

```rust
struct DedupIndex {
    // Tier 1: Message-ID → first occurrence
    message_ids: HashMap<String, MessageRef>,
    // Tier 2: content hash → first occurrence
    content_hashes: HashMap<[u8; 32], MessageRef>,
}

struct MessageRef {
    pst_file: usize,        // index into input PST list
    folder_path: String,     // e.g., "Inbox/Projects"
    nid: u64,               // message NID for re-extraction
    subject: String,
    date: Option<i64>,
    sender: String,
    size: u32,
}

enum DedupResult {
    Unique,
    DuplicateOf { original: MessageRef, tier: DedupTier },
}

enum DedupTier { MessageId, ContentHash }
```

### Processing Pipeline

```
For each PST file (sequential — PST files aren't safe to mmap concurrently):
  1. Open file, parse header, validate
  2. Build NBT + BBT index (full B-tree traversal, cache in memory)
  3. Walk folder hierarchy from NID_ROOT_FOLDER
  4. For each message NID in each folder's contents table:
     a. Read message PC (lightweight — just the properties we need)
     b. Compute Tier 1 key (Message-ID)
     c. If Tier 1 key exists → check index → duplicate or unique
     d. If no Message-ID → compute Tier 2 hash → check index
     e. Record result in DedupResult vec
     f. Emit progress update to GUI channel
```

### Memory Budget

At 1M messages, the index is approximately:
- Message-ID strings: ~80 bytes avg × 1M = ~80MB
- MessageRef structs: ~200 bytes × 1M = ~200MB
- HashMap overhead: ~50% = ~140MB
- **Total: ~420MB** — fits comfortably in a workstation

The NBT/BBT indexes are smaller (tens of thousands of entries per PST).

---

## GUI Design (egui)

### Dedupe Desk (primary — 0020)

```
Home (create/open/recent) → Workspace (sources / process / jobs)
                         ↘ stub nav: Reduce / Review / Produce (later tracks)
```

Worker ownership: `ProcessRunner` (0019). Progress: `watch` borrow each frame.
Legacy scan wizard below is retained for engine regression only.

### Legacy wizard state machine (`pst-dedup-gui`)

```
FileSelect → Settings → Scanning → Results
    ↑                        ↓ (cancel)
    ←←←←←←←←←←←←←←←←←←←←←←←
```

### FileSelect View
- "Add PST Files" button → native file dialog (rfd crate)
- List of selected files with size, remove button
- "Next →" enabled when ≥1 file selected

### Settings View  
- Checkboxes: Enable Tier 1 (always on), Enable Tier 2
- Tier 2 body hash length slider (1KB–8KB, default 4KB)
- Include attachment metadata in hash: checkbox
- Output directory picker
- "Start Scan →"

### Progress View
- Overall progress bar (messages processed / estimated total)
- Per-file progress bar for current PST
- Stats: messages/sec, unique count, duplicate count, elapsed time
- "Cancel" button → sets cancellation flag, worker thread checks it

### Results View
- Summary stats: total scanned, unique, duplicates (by tier), savings estimate
- Scrollable table: Subject, Date, Sender, Tier, Original PST, Duplicate PSTs
- Filters: by tier, by PST, search by subject/sender
- "Export CSV Report" button
- "Export Unique Emails (EML)" button with progress

---

## Implementation Phases

### Phase 1: NDB Foundation
**Goal:** Read any Unicode PST's complete node and block tree.

1. `header.rs` — parse + validate header, extract ROOT, crypto method
2. `ndb/nid.rs` — NID types, special NID constants
3. `ndb/page.rs` — read 512-byte pages, validate trailers
4. `ndb/btree.rs` — recursive B-tree traversal for NBT and BBT
5. `ndb/block.rs` — data block reading, XBLOCK/XXBLOCK assembly, decryption
6. `crypto.rs` — permute table + cyclic decoder
7. **Validation:** open a test PST, dump all NID→BID mappings, verify block reads

### Phase 2: LTP Properties
**Goal:** Read property values from any node.

1. `ltp/hn.rs` — Heap-on-Node: parse HNHDR, resolve HIDs to byte slices
2. `ltp/bth.rs` — BTree-on-Heap: traverse and lookup by key
3. `ltp/pc.rs` — Property Context: resolve property tags to typed values
4. `ltp/tc.rs` — Table Context: iterate rows, extract column values
5. **Validation:** read Message Store properties (display name, etc.)

### Phase 3: Messaging
**Goal:** Iterate folders and extract message properties.

1. `messaging/store.rs` — open message store, named property map (basic)
2. `messaging/folder.rs` — recursive folder walker using hierarchy TCs
3. `messaging/message.rs` — extract dedup-relevant properties from message PCs
4. `messaging/attachment.rs` — attachment table iteration for name+size
5. **Validation:** dump all messages from a test PST with subject/date/sender

### Phase 4: Dedup Engine
**Goal:** Full dedup pipeline producing report.

1. `dedup-engine/hasher.rs` — Tier 1 + Tier 2 hashing
2. `dedup-engine/index.rs` — DedupIndex with insert + check
3. `dedup-engine/report.rs` — CSV report writer
4. `dedup-engine/exporter.rs` — EML export (RFC 5322 format)
5. **Validation:** dedup two PSTs with known overlapping emails

### Phase 5: GUI
**Goal:** Ship the interactive surface.

1. `pst-dedup-gui/app.rs` — state machine, channel-based worker communication
2. `pst-dedup-gui/worker.rs` — background thread running the pipeline
3. `pst-dedup-gui/views/*` — four views matching the state machine
4. Windows-specific: set icon, metadata, `#![windows_subsystem = "windows"]`
5. **Validation:** end-to-end on real DOC PST files

### Phase 5b: CLI
**Goal:** Agent- and script-friendly surface (done for core commands).

1. `pst-dedup-cli` binary `pst-dedup` — `inspect` / `scan` / `dups`
2. `--json` and `--csv` outputs; logs on stderr
3. **Validation:** real multi-mailbox Permute PST scan matches engine counts

### Phase 6: Hardening
- CRC32 validation on pages and blocks (warning-only today; algorithm still under review)
- Graceful handling of corrupted PSTs (skip bad nodes, log warnings)
- Large file testing (>10GB PSTs)
- Progress estimation improvement (pre-scan contents table counts)

---

## Dependencies (all permissive)

```toml
[workspace.dependencies]
eframe = "0.34"           # egui framework (MIT/Apache-2.0)
sha2 = "0.11"             # SHA-256 hashing (MIT/Apache-2.0)
csv = "1.4"               # CSV report writing (MIT/Unlicense)
rfd = "0.17"              # Native file dialogs (MIT)
chrono = "0.4"            # Date formatting (MIT/Apache-2.0)
byteorder = "1.5"         # Little-endian reads (MIT/Unlicense)
thiserror = "2"           # Error types (MIT/Apache-2.0)
tracing = "0.1"           # Logging (MIT)
tracing-subscriber = "0.3"
crc32fast = "1.5"         # CRC32 validation (MIT/Apache-2.0)
```

No LGPL, no GPL, no viral licenses. All compatible with government commercial use.

---

## Test Strategy

### Unit Tests
- Header parsing: craft byte arrays for valid/invalid headers
- Crypto: known-plaintext test vectors for permute and cyclic
- BTree traversal: mock page data with known structure
- Property extraction: hand-built PC/TC node data

### Integration Tests
- Use small PSTs created by Outlook for testing (include in `tests/fixtures/`)
- Microsoft provides sample PST files in their interop documentation
- Create synthetic PSTs with known duplicate patterns

### Performance Benchmarks
- `criterion` benchmarks for:
  - Block read + decrypt throughput
  - BTree traversal speed
  - Dedup hash computation rate
  - End-to-end: messages/second on a 100K message PST

---

## File Format Quick Reference

```
PST File Layout (Unicode):
┌─────────────────────────────────────┐
│ Header (564 bytes padded to 4096)   │  ← dwMagic, ROOT, crypto method
├─────────────────────────────────────┤
│ Allocation Map pages (AMap)         │  ← tracks free space (can skip for read-only)
├─────────────────────────────────────┤
│ Data blocks (8KB aligned)           │  ← node data, encrypted
├─────────────────────────────────────┤
│ BTree pages (512 bytes each)        │  ← NBT and BBT pages, interleaved
├─────────────────────────────────────┤
│ More data blocks and pages...       │  ← file grows as needed
└─────────────────────────────────────┘

Read path for a message property:
  Header → ROOT.BREFNBT → NBT root page
    → traverse NBT to find message NID → get bidData, bidSub
      → BBT lookup bidData → get block location (ib)
        → read block(s) → decrypt → HN → BTH → PC
          → property tag lookup → value (inline or HID or subnode)
```
