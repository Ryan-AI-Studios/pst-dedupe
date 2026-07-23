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
| Fixed MS-PST template-object tables | Yes, always empty | Hierarchy/Contents/AssocContents/SearchContents Table Templates at fixed NIDs `0x60D`/`0x60E`/`0x60F`/`0x610`; zero data rows, correct column schema. |
| Associated-contents (FAI) table | Yes, empty | Root, IPM_SUBTREE, `<folder>`, Deleted Items, and Search Root each get an empty associated-contents TC (NID suffix `0x0F`) alongside their PC/hierarchy/contents TCs — MS-PST §2.4.2 completeness; see below. No FAI items are ever written in v1. |
| Attachments | **No** (dropped) | Owned by track **0069**. `HasAttachments` is always `false`. `from_canonical_message` reports a per-message dropped-attachment count. |
| Folder path preservation | **No** (flat `<folder>` under IPM_SUBTREE) | Owned by **0069**. |
| Multi-source prefixes | **No** | Owned by **0069**. |
| Multi-GB streaming write | **No** — v1 collects `WriteMessage`s and builds the whole layout in memory (two-pass), suitable for synthetic/thousands-of-messages scale | Owned by **0070**. |
| Encrypted / Permute output | **No** | Residual; unencrypted only. |
| ANSI PST | **No** | Never. |
| Recipient table / named-prop set beyond the store stub | **No** | Minimal named-property map stub only, sufficient for the properties this writer emits (none of which require named props). |
| RTF | **No** | v1 never writes `PidTagRtfCompressed` or any RTF-native hint — there is nothing RTF-related to clear because nothing RTF-related is ever produced. |
| `PidTagMessageFlags` | Always `0x00000001` (`MSGFLAG_READ`) | Cheap, low-risk fidelity addition beyond the §3.3 LOCKED table; a sane constant default, not a real read/unread feature. |
| `PidTagCreationTime` / `PidTagLastModificationTime` | Set to `submit_time` when present; omitted otherwise | This is a synthetically-written export item, not a live mailbox object, so `submit_time` is a defensible stand-in for both when no better source exists. Never invented — omitted entirely when `submit_time` is `None`. |

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
  NID_SEARCH_CONTENTS_TABLE_TEMPLATE 0x610) — always zero data rows
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
`generate_store_record_key()` (`crates/pst-writer/src/production.rs`) and
reused, byte-for-byte, as the EntryID's ProviderUID above. This closes a
round-5 cross-model review finding: earlier v1 wrote no `PidTagRecordKey` at
all and hardcoded the EntryID's ProviderUID to an arbitrary all-zero
placeholder. A store-internal EntryID's provider UID is conventionally the
store's own unique record key, not an arbitrary value — the fix makes the
EntryID genuinely self-consistent and identifies this specific store, rather
than pointing at a degenerate zero placeholder.

`generate_store_record_key()` is a best-effort unique identifier, **not** a
cryptographic GUID: per this crate's minimal-dependency convention (no
`uuid`, no `rand`), it derives the 16 bytes from write-time-varying inputs
already available (`SystemTime::now()` nanoseconds, `std::process::id()`, the
destination path, and the message count) hashed four times with
`crc32fast::hash` under different salts. It only guarantees non-zero,
self-consistent (same bytes in `PidTagRecordKey` and the EntryID), and
reasonably-unique-per-invocation — never a cryptographically strong or
globally unique value.

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
`store_record_key_present_nonzero_and_matches_entry_id_provider_uid`); and two
separate writes produce different record keys, proving the value is genuinely
generated per write rather than a hardcoded constant (see
`store_record_key_differs_across_separate_writes`).

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

A single HN heap allocation cannot span more than one heap page — this is
inherent to the MS-PST Heap-on-Node format (`HNPAGEMAP` offsets are local to
one physical block), not a writer shortcut. Any `body_plain` (UTF-16LE bytes)
or `body_html` value larger than **3580 bytes** is written as a **subnode**
(MS-PST §2.3.3.3) instead of an inline heap allocation, referenced by NID
rather than HID. `pst-reader`'s `PropContext` did not previously resolve
subnode-typed HNIDs for `PtypString`/`PtypBinary` (it silently returned
`None`), which would have blocked round-trip verification of large bodies —
that gap was fixed in `pst_reader::ltp::pc` as part of this track (see that
module's doc comments and the "reader compatibility" note in the final
implementation report), per the explicit allowance to fix a genuine reader bug
blocking round-trip verification rather than working around it in the writer.

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
this; instead it follows its own established `generate_store_record_key`
pattern (see that function's docs) for dependency-free entropy: `<entropy>`
is an 8-hex-digit `crc32fast::hash` over wall-clock nanoseconds since the
epoch plus the process ID, computed once per process and cached
(`process_entropy_suffix`) so repeated calls for the same destination within
one run — including the integration test calling `temp_sibling_path`
directly to predict the exact value `write_unicode_pst` will compute —
agree, while a different process (a later run, a restarted one, or an
attacker without this process's PID/start time) gets a different suffix.
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
  diverts them to a subnode first (body/HTML — the only genuinely unbounded
  fields on `WriteMessage`).

## CanonicalMessage → WriteMessage adapter

`pst-writer` takes a normal crate dependency on `dedup-engine` for exactly one
function, `pst_writer::from_canonical_message(&CanonicalMessage) -> (WriteMessage, u64)`.
No cycle is introduced (`dedup-engine` does not depend on `pst-writer`).
Attachments are dropped; the `u64` is the number of attachments dropped for
that message, for a caller (e.g. a future 0071 CLI) to aggregate into a report.

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
