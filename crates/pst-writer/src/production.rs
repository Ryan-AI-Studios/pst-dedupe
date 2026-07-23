//! Production Unicode PST writer v1 (Ledgerful track 0068).
//!
//! Evolves the fixture-scale writer (`crate::write_pst_from_emls`, kept as-is for
//! existing callers) into a writer that can emit a **valid Unicode, unencrypted
//! PST** for keep-set winners, with:
//!
//! - Full plain/HTML bodies via XBLOCK/XXBLOCK (no silent 2000-char truncate).
//! - A real `Root → IPM_SUBTREE → <folder>` hierarchy with a store
//!   `PidTagIpmSubtreeEntryId` (§3.2).
//! - `PidTagNativeBody` / `PidTagMessageEditorFormat` / `PidTagInternetCodepage`
//!   set to match what was actually written (§3.3.1) — never stale RTF hints
//!   (v1 never writes RTF at all).
//! - `PidTagMessageSize` computed from bytes actually written, never copied from
//!   a (possibly inflated) source size (§3.3.2).
//! - `Result`-only allocation: nothing in this module's call graph reaches the
//!   fixture path's `assert!`-based `Layout::add_node`.
//!
//! ## Large single-property values: subnode storage (not silent truncation)
//!
//! A single HN heap allocation cannot span more than one heap page (the
//! `HNPAGEMAP` for each page only knows offsets local to that page — see
//! `pst_reader::ltp::hn::Heap::get`). This is inherent to the MS-PST format, not
//! a writer shortcut: MS-PST §2.3.3.3 requires values that don't fit a heap page
//! to be moved to a **subnode** instead, referenced by NID rather than HID. Any
//! `body_plain` / `body_html` value larger than [`MAX_HEAP_VALUE_SIZE`] is written
//! this way. `pst-reader`'s `PropContext` did not previously resolve subnode-typed
//! HNIDs for PtypString/PtypBinary (it silently returned `None`), which would have
//! blocked round-trip verification of large bodies — that gap was fixed in
//! `pst_reader::ltp::pc` (see its module docs) as part of this track, per the
//! explicit allowance to fix a genuine reader bug blocking round-trip
//! verification rather than working around it in the writer.
//!
//! ## Scope (v1)
//!
//! No attachments, no folder-path preservation, no multi-GB streaming (all owned
//! by 0069/0070). See `docs/pst-writer-fidelity-v1.md`.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use byteorder::{LittleEndian, WriteBytesExt};

use crate::{
    write_data_block, BlockEntry, HeapBuilder, Layout, NodeEntry, Result, WriterError,
    CLIENT_MAGIC, HEADER_SIZE, MAX_BLOCK_DATA, NID_ASSOC_CONTENTS_TABLE_TEMPLATE,
    NID_CONTENTS_TABLE_TEMPLATE, NID_HIERARCHY_TABLE_TEMPLATE, NID_MESSAGE_STORE,
    NID_NAME_TO_ID_MAP, NID_ROOT_FOLDER, NID_SEARCH_CONTENTS_TABLE_TEMPLATE,
    NID_TYPE_NORMAL_FOLDER, NID_TYPE_NORMAL_MESSAGE, NID_TYPE_SEARCH_FOLDER, PAGE_SIZE,
    PID_TAG_CLIENT_SUBMIT_TIME, PID_TAG_CONTENT_COUNT, PID_TAG_DISPLAY_NAME,
    PID_TAG_HAS_ATTACHMENTS, PID_TAG_INTERNET_MESSAGE_ID, PID_TAG_LTP_ROW_ID,
    PID_TAG_SENDER_EMAIL_ADDRESS, PID_TAG_SUBJECT, PST_MAGIC, PTYP_BOOLEAN, PTYP_INTEGER_32,
    PTYP_INTEGER_64, PTYP_STRING, PTYP_TIME, UNICODE_VERSION,
};

// ── New property tags needed for the production path ────────────────────────

const PID_TAG_MESSAGE_CLASS: u16 = 0x001A;
const PID_TAG_MESSAGE_FLAGS: u16 = 0x0E07;
const PID_TAG_CREATION_TIME: u16 = 0x3007;
const PID_TAG_LAST_MODIFICATION_TIME: u16 = 0x3008;
const PID_TAG_DISPLAY_TO: u16 = 0x0E04;
const PID_TAG_NATIVE_BODY: u16 = 0x1016;
const PID_TAG_MESSAGE_EDITOR_FORMAT: u16 = 0x5909;
const PID_TAG_INTERNET_CODEPAGE: u16 = 0x3FDE;
const PID_TAG_BODY_HTML: u16 = 0x1013;
const PID_TAG_BODY: u16 = 0x1000;
const PID_TAG_MESSAGE_SIZE: u16 = 0x0E08;
const PID_TAG_IPM_SUBTREE_ENTRYID: u16 = 0x35E0;
/// PidTagIpmWastebasketEntryId (MS-PST §5493a0eb, "Minimum Set of Required
/// Properties" for a message store PC — track 0068 round 9, verified).
const PID_TAG_IPM_WASTEBASKET_ENTRYID: u16 = 0x35E3;
/// PidTagFinderEntryId (same source as above).
const PID_TAG_FINDER_ENTRYID: u16 = 0x35E7;
const PID_TAG_CONTAINER_CLASS: u16 = 0x3613;
const PID_TAG_RECORD_KEY: u16 = 0x0FF9;
/// PidTagContentUnreadCount — required IPM_SUBTREE initialization (MS-PST
/// §ea4d8b8a, "Top of Personal Folders" schema — track 0068 round 9).
const PID_TAG_CONTENT_UNREAD_COUNT: u16 = 0x3603;
/// PidTagSubfolders — same source as above.
const PID_TAG_SUBFOLDERS: u16 = 0x360A;
const PTYP_BINARY: u16 = 0x0102;
/// PtypMultipleInteger32 — used only by the FAI Contents Table Template's
/// `PidTagFlatUrgency`-shaped column (0x6805). This repo's TC column model has
/// no existing precedent for a genuine PtypMultiple* value; per the verified
/// source data's own guidance, its row-column width is conservatively modeled
/// as a 4-byte HNID reference (like `PtypString`/`PtypBinary`), never as an
/// inline fixed-size value — this table has zero rows in v1 regardless, so
/// only the TCOLDESC byte-width bookkeeping is exercised, not real multi-value
/// storage/decoding. Documented judgment call — see final report.
const PTYP_MULTIPLE_INTEGER_32: u16 = 0x1003;

/// Above this size (bytes, post-encoding) a PtypString/PtypBinary value is moved
/// to a subnode instead of inlined in the HN heap (see module docs). Chosen with
/// generous headroom under one heap page (~8100 usable bytes) so the handful of
/// other small message properties always fit alongside it.
const MAX_HEAP_VALUE_SIZE: usize = 3580;

/// Max BID entries in one XBLOCK/XXBLOCK: `(MAX_BLOCK_DATA - 8) / 8`.
const MAX_XBLOCK_ENTRIES: usize = (MAX_BLOCK_DATA - 8) / 8;

/// BTree intermediate/leaf-of-BBT entry size (key+BREF or BBTENTRY), used to size
/// how many child references fit in the 488-byte entries region of one page.
const INTERMEDIATE_ENTRY_SIZE: usize = 24;
const INTERMEDIATE_ENTRIES_PER_PAGE: usize = 488 / INTERMEDIATE_ENTRY_SIZE;

const NBT_LEAF_ENTRY_SIZE: usize = 32;
const BBT_LEAF_ENTRY_SIZE: usize = 24;

const PTYPE_BBT: u8 = 0x80;
const PTYPE_NBT: u8 = 0x81;
const PTYPE_AMAP: u8 = 0x84;

// ── Public API (spec §3.6) ───────────────────────────────────────────────────

/// A plain message DTO the production writer consumes. Deliberately independent
/// of `dedup_engine::CanonicalMessage` — see [`from_canonical_message`] for the
/// mapping adapter (attachments dropped; v1 owns no attachment fidelity).
#[derive(Debug, Clone, Default)]
pub struct WriteMessage {
    pub message_id: Option<String>,
    pub subject: String,
    pub sender: Option<String>,
    pub display_to: Option<String>,
    /// Absolute FILETIME passthrough (100ns since 1601-01-01), if present.
    pub submit_time: Option<i64>,
    pub body_plain: Option<String>,
    pub body_html: Option<Vec<u8>>,
    pub message_class: Option<String>,
    /// Fidelity flag for reporting only — never written as a MAPI property in v1.
    pub body_incomplete: bool,
    /// Fidelity flag for reporting only — when true, no body is written at all
    /// (never invented) regardless of `body_plain`/`body_html` contents.
    pub body_unavailable: bool,
}

/// Options for [`write_unicode_pst`].
#[derive(Debug, Clone)]
pub struct WritePstOpts {
    /// Display name of the single flat mail folder under IPM_SUBTREE.
    pub folder_display_name: String,
    /// Safety gate (§3.7 rule 3): by default `write_unicode_pst` refuses to
    /// write when `path` already exists. Set `true` to explicitly allow
    /// replacing it. This knob only ever governs **stale output** the caller
    /// is allowed to clobber — it never overrides the `protected_source_paths`
    /// function parameter of [`write_unicode_pst`], which is a separate,
    /// non-overridable rule (§3.7 rule 2). `write_unicode_pst` never mutates
    /// an existing file in place either way — it always writes a fresh temp
    /// file and renames over the destination on success (Windows `rename`
    /// replaces the target).
    pub overwrite: bool,
}

impl Default for WritePstOpts {
    fn default() -> Self {
        Self {
            folder_display_name: "Unique Mail".to_string(),
            overwrite: false,
        }
    }
}

/// Best-effort normalized comparison path for output-safety checks: prefers
/// `canonicalize()` (resolves symlinks/relative components/case on Windows),
/// but falls back to the path as given when canonicalization fails — which is
/// expected and normal for a destination that does not exist yet. This must
/// never be used to *grant* access, only to compare two paths for equality, so
/// a fallback that's merely "less normalized" (not "insecure") is acceptable.
fn normalize_for_comparison(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// Shared enforcement for spec §3.7 rule 2 / Core Mandate #3: refuse (typed
/// [`WriterError::RefusedSourceOverwrite`]) if `candidate` matches any entry
/// in `protected_source_paths`, by the same best-effort canonicalized
/// comparison used everywhere else in this module. Factored out of
/// `write_unicode_pst` so the exact same logic — not a hand-copied variant
/// that could quietly drift — is applied both to the final destination path
/// and to the temp-staging path (see `temp_sibling_path`): the temp path is
/// where bytes are *actually* written first, via `File::create`, so it needs
/// the identical protection, applied before that call, not just the final
/// rename target.
fn check_not_protected_source(candidate: &Path, protected_source_paths: &[PathBuf]) -> Result<()> {
    let normalized = normalize_for_comparison(candidate);
    if protected_source_paths
        .iter()
        .any(|src| normalize_for_comparison(src) == normalized)
    {
        return Err(WriterError::RefusedSourceOverwrite(candidate.to_path_buf()));
    }
    Ok(())
}

/// Result of a successful [`write_unicode_pst`] call.
#[derive(Debug, Clone)]
pub struct WritePstReport {
    pub messages_written: u64,
    /// Always 0 in v1: any per-message hard error fails the whole write rather
    /// than silently omitting a message (see module docs / final report).
    pub messages_skipped: u64,
    pub path: PathBuf,
    pub bytes: u64,
    /// Count of written messages whose source `WriteMessage.body_incomplete`
    /// was `true` (spec §2.4: written with available props + partial body,
    /// never invented — this surfaces how many in the write report).
    pub messages_with_incomplete_body: u64,
    /// Count of written messages whose source `WriteMessage.body_unavailable`
    /// was `true` (written with no body at all, never invented — this
    /// surfaces how many in the write report).
    pub messages_with_unavailable_body: u64,
}

/// Map a `CanonicalMessage` (0066 keep-set winner) to the plain `WriteMessage`
/// DTO this writer consumes.
///
/// Design choice (documented per spec §3.6): rather than adding an adapter crate
/// or duplicating the mapping in every caller, `pst-writer` takes a normal
/// dependency on `dedup-engine` for exactly this one free function — `pst-writer`
/// never depends on `pst-dedup-cli`, and `dedup-engine` never depends back on
/// `pst-writer`, so no cycle is introduced. Attachments are dropped (owned by
/// 0069); the second return value is the number of attachments dropped for this
/// message so a caller (e.g. a future 0071 CLI) can aggregate a report — this
/// writer's own `WritePstReport` has no attachment concept since `WriteMessage`
/// carries none.
pub fn from_canonical_message(
    msg: &dedup_engine::keepset::CanonicalMessage,
) -> (WriteMessage, u64) {
    let dropped = msg.attachments.len() as u64;
    let write_msg = WriteMessage {
        message_id: msg.message_id.clone(),
        subject: msg.subject.clone().unwrap_or_default(),
        sender: msg.sender.clone(),
        display_to: msg.display_to.clone(),
        submit_time: msg.submit_time,
        body_plain: msg.body_plain.clone(),
        body_html: msg.body_html.clone(),
        message_class: msg.message_class.clone(),
        body_incomplete: msg.body_incomplete,
        body_unavailable: msg.body_unavailable,
    };
    (write_msg, dropped)
}

/// Write a production-scope Unicode, unencrypted PST containing `messages`.
///
/// See module docs and `docs/pst-writer-fidelity-v1.md` for what v1 does and
/// does not do. Writes to a `.tmp-<pid>-<entropy>` sibling of `path` (see
/// `temp_sibling_path`) and renames over `path` only after the full file is
/// written successfully (§3.7).
///
/// ## Why `protected_source_paths` is a mandatory function parameter, not a
/// field on `WritePstOpts`
///
/// It used to be a `WritePstOpts` field defaulting to `Vec::new()`. That made
/// it trivially easy to get **zero** source-overwrite protection completely
/// silently: `WritePstOpts::default()` and `WritePstOpts { overwrite: true,
/// ..Default::default() }` are both completely ordinary, easy-to-write
/// patterns, and neither of them raises any compiler warning, runtime
/// warning, or friction of any kind — the protection only existed if the
/// caller happened to remember to populate that one specific field. Making it
/// a required, separate function parameter instead means every call site must
/// type *something* for it, even a deliberately empty `&[]` — that is a
/// conscious, visible choice to opt out of protection, not an invisible
/// default. This crate deliberately does not parse or track source PSTs
/// itself (that is the caller's — e.g. a future 0069/0071 CLI's —
/// responsibility), so this can never be *complete* enforcement: the library
/// still has no way to verify the caller passed the right paths, or all of
/// them. That residual trust boundary is inherent to any library that
/// doesn't independently know its caller's inputs, and is stated here
/// plainly rather than hidden behind a struct field that looks like it
/// "just works" when left at its default.
///
/// Two independent output-safety checks (§3.7):
/// 1. **Always**, regardless of `opts.overwrite`: refuses (typed
///    [`WriterError::RefusedSourceOverwrite`]) if `path` matches any entry in
///    `protected_source_paths` — this project never mutates PST inputs
///    (Core Mandate #3), and no opt-in can override it. **This same check is
///    also applied to the computed temp-staging path** (see
///    `temp_sibling_path`), before that path is ever passed to
///    `File::create` — the temp path is where bytes are *actually* written
///    first, so checking only the final rename target would be an incomplete
///    promise: a source PST that happened to collide with the temp name
///    would otherwise be silently truncated during staging, before the
///    rename step that the final-path check guards. See `temp_sibling_path`
///    for how its name is derived to make that collision unlikely in the
///    first place, on top of this explicit check.
/// 2. By default (unless `opts.overwrite` is `true`): refuses (typed
///    [`WriterError::Refused`]) to overwrite an existing `path` at all — this
///    one *can* be legitimately overridden, since it only ever concerns stale
///    **output** the caller is allowed to clobber.
///
/// It never mutates an existing file in place either way.
pub fn write_unicode_pst(
    path: &Path,
    messages: impl IntoIterator<Item = WriteMessage>,
    protected_source_paths: &[PathBuf],
    opts: &WritePstOpts,
) -> Result<WritePstReport> {
    check_not_protected_source(path, protected_source_paths)?;

    if path.exists() && !opts.overwrite {
        return Err(WriterError::Refused(format!(
            "destination {} already exists; pass WritePstOpts {{ overwrite: true, .. }} to replace \
             it (pst-writer never overwrites by default and never mutates an existing PST in place)",
            path.display()
        )));
    }

    let messages: Vec<WriteMessage> = messages.into_iter().collect();
    let mut layout = Layout::new();

    // ── Named property map (stub; minimal for open) ──────────────────────────
    let named_heap = {
        let mut heap = HeapBuilder::new(0x6C);
        let hid = build_pc_v2(&mut heap, &[])?;
        heap.finalize(hid)
    };
    layout.add_node_data(NID_NAME_TO_ID_MAP, named_heap, 0, 0)?;

    // ── Root folder → IPM_SUBTREE → <folder> hierarchy (§3.2) ────────────────
    let ipm_subtree_nid = layout.alloc_nid(NID_TYPE_NORMAL_FOLDER);

    let root_heap = {
        let mut heap = HeapBuilder::new(0x6C);
        let hid = build_pc_v2(
            &mut heap,
            &[
                (PID_TAG_DISPLAY_NAME, PcValue::String("Root".to_string())),
                (PID_TAG_CONTENT_COUNT, PcValue::I32(0)),
            ],
        )?;
        heap.finalize(hid)
    };
    layout.add_node_data(NID_ROOT_FOLDER, root_heap, 0, 0)?;

    let root_hier_heap = {
        let mut heap = HeapBuilder::new(0xBC);
        let columns = [(PID_TAG_LTP_ROW_ID, PTYP_INTEGER_32, 0u16, 4u8, 0u8)];
        let rows = vec![(ipm_subtree_nid as u32).to_le_bytes().to_vec()];
        let hid = build_tc_inline_checked(&mut heap, &columns, &rows)?;
        heap.finalize(hid)
    };
    layout.add_node_data((NID_ROOT_FOLDER & !0x1F) | 0x0D, root_hier_heap, 0, 0)?;

    let root_cont_heap = {
        let mut heap = HeapBuilder::new(0xBC);
        let columns = [(PID_TAG_LTP_ROW_ID, PTYP_INTEGER_32, 0u16, 4u8, 0u8)];
        let hid = build_tc_inline_checked(&mut heap, &columns, &[])?;
        heap.finalize(hid)
    };
    layout.add_node_data((NID_ROOT_FOLDER & !0x1F) | 0x0E, root_cont_heap, 0, 0)?;

    // Associated-contents (FAI) table, empty (§ MS-PST 2.4.2 — a complete
    // Folder object is PC + hierarchy TC + contents TC + associated-contents
    // TC, even when the latter is empty; codex round-6 P1 finding, Item 2).
    // NID type suffix 0x0F: confirmed against this repo's own canonical
    // NID-type scheme in `pst_reader::ndb::nid::NodeId::associated_contents_table`
    // (`(self.0 & !0x1F) | 0x0F`) and `NidType::AssocContentsTable`, not guessed.
    let root_assoc_cont_heap = {
        let mut heap = HeapBuilder::new(0xBC);
        let columns = [(PID_TAG_LTP_ROW_ID, PTYP_INTEGER_32, 0u16, 4u8, 0u8)];
        let hid = build_tc_inline_checked(&mut heap, &columns, &[])?;
        heap.finalize(hid)
    };
    layout.add_node_data((NID_ROOT_FOLDER & !0x1F) | 0x0F, root_assoc_cont_heap, 0, 0)?;

    let unique_mail_nid = layout.alloc_nid(NID_TYPE_NORMAL_FOLDER);
    // Deleted Items / Search Root (§2/§3/§4 of the round-9 verified MS-PST
    // data — supersedes the prior D-0068-05 decline, see
    // `docs/pst-writer-fidelity-v1.md`). Allocated here, alongside
    // `unique_mail_nid`, so both are available before the IPM_SUBTREE
    // hierarchy TC (which references Deleted Items) and the message store PC
    // (which references both via `PidTagIpmWastebasketEntryId` /
    // `PidTagFinderEntryId`) are built below.
    let deleted_items_nid = layout.alloc_nid(NID_TYPE_NORMAL_FOLDER);
    // NID_TYPE_SEARCH_FOLDER (0x03) — verified from
    // https://learn.microsoft.com/en-us/openspecs/office_file_formats/ms-pst/2dfb3012-b81c-466b-831c-2d2f0c29e591:
    // "The search Folder object is implemented as a PC that is identified by
    // a special NID_TYPE of NID_TYPE_SEARCH_FOLDER (0x03)." Not a child of
    // IPM_SUBTREE's hierarchy TC (the verified "Top of Personal Folders"
    // hierarchy-TC row list names only Deleted Items) — referenced solely via
    // the store's `PidTagFinderEntryId` below, so `nid_parent` is 0 like the
    // other top-level objects (store/root/named-prop-map/templates).
    let search_root_nid = layout.alloc_nid(NID_TYPE_SEARCH_FOLDER);

    // `PidTagContainerClass` (0x3613) is deliberately NOT set on the
    // IPM_SUBTREE folder itself (§3.2, review round 3 P2). Real-world
    // Unicode PSTs generated by Outlook leave the IPM_SUBTREE ("Top of
    // Personal Folders") node's own `PidTagContainerClass` absent/empty —
    // the container class convention (MS-PST/MAPI: `IPF.Note`, `IPF.Contact`,
    // etc.) exists to tell Outlook what *kind of items* a leaf mail-holding
    // folder contains, not to classify the subtree root itself, which has no
    // single item type. It is set instead on the "Unique Mail" folder below,
    // the actual folder that holds `IPM.Note` messages — see that folder's
    // PC build and `docs/pst-writer-fidelity-v1.md` for the full reasoning.
    //
    // DisplayName/ContentCount/ContentUnreadCount/Subfolders values below are
    // the exact required initialization values verified (round 9) from
    // https://learn.microsoft.com/en-us/openspecs/office_file_formats/ms-pst/ea4d8b8a-6062-4930-94ee-555527a274d1
    // ("Top of Personal Folders" / IPM_SUBTREE schema-properties table) —
    // this supersedes the prior literal-string bug where this folder's
    // PidTagDisplayName was written as "IPM_SUBTREE" instead of the
    // MS-PST-required "Top of Personal Folders".
    let ipm_heap = {
        let mut heap = HeapBuilder::new(0x6C);
        let hid = build_pc_v2(
            &mut heap,
            &[
                (
                    PID_TAG_DISPLAY_NAME,
                    PcValue::String("Top of Personal Folders".to_string()),
                ),
                (PID_TAG_CONTENT_COUNT, PcValue::I32(1)),
                (PID_TAG_CONTENT_UNREAD_COUNT, PcValue::I32(0)),
                (PID_TAG_SUBFOLDERS, PcValue::Bool(true)),
            ],
        )?;
        heap.finalize(hid)
    };
    layout.add_node_data(ipm_subtree_nid, ipm_heap, NID_ROOT_FOLDER, 0)?;

    // Hierarchy TC: verified (round 9) source data's "Hierarchy TC" row list
    // for IPM_SUBTREE names "Deleted Items"; this track's own LOCKED v1 shape
    // (spec.md §3.2) additionally puts "Unique Mail" here — both rows are
    // kept, per the explicit instruction to add Deleted Items alongside the
    // existing Unique Mail row rather than replace it.
    let ipm_hier_heap = {
        let mut heap = HeapBuilder::new(0xBC);
        let columns = [(PID_TAG_LTP_ROW_ID, PTYP_INTEGER_32, 0u16, 4u8, 0u8)];
        let rows = vec![
            (unique_mail_nid as u32).to_le_bytes().to_vec(),
            (deleted_items_nid as u32).to_le_bytes().to_vec(),
        ];
        let hid = build_tc_inline_checked(&mut heap, &columns, &rows)?;
        heap.finalize(hid)
    };
    layout.add_node_data((ipm_subtree_nid & !0x1F) | 0x0D, ipm_hier_heap, 0, 0)?;

    let ipm_cont_heap = {
        let mut heap = HeapBuilder::new(0xBC);
        let columns = [(PID_TAG_LTP_ROW_ID, PTYP_INTEGER_32, 0u16, 4u8, 0u8)];
        let hid = build_tc_inline_checked(&mut heap, &columns, &[])?;
        heap.finalize(hid)
    };
    layout.add_node_data((ipm_subtree_nid & !0x1F) | 0x0E, ipm_cont_heap, 0, 0)?;

    // Associated-contents (FAI) table, empty — see the Root folder's comment
    // above for the MS-PST §2.4.2 rationale and NID-suffix cross-check.
    let ipm_assoc_cont_heap = {
        let mut heap = HeapBuilder::new(0xBC);
        let columns = [(PID_TAG_LTP_ROW_ID, PTYP_INTEGER_32, 0u16, 4u8, 0u8)];
        let hid = build_tc_inline_checked(&mut heap, &columns, &[])?;
        heap.finalize(hid)
    };
    layout.add_node_data((ipm_subtree_nid & !0x1F) | 0x0F, ipm_assoc_cont_heap, 0, 0)?;

    // ── Deleted Items folder (§3 of the round-9 verified MS-PST data) ───────
    //
    // Same PC + hierarchy TC (empty) + contents TC (empty) + associated-
    // contents TC (empty) shape as "Unique Mail" below, per the exact
    // instruction: create it "exactly like the existing Unique Mail folder".
    // v1 never invents deleted-items content — this folder is always empty;
    // it exists to satisfy the verified MS-PST structural requirement (a
    // hierarchy-TC row under IPM_SUBTREE) and to give
    // `PidTagIpmWastebasketEntryId` (on the message store PC, below) a real
    // folder to reference instead of a dangling NID.
    let deleted_items_heap = {
        let mut heap = HeapBuilder::new(0x6C);
        let hid = build_pc_v2(
            &mut heap,
            &[
                (
                    PID_TAG_DISPLAY_NAME,
                    PcValue::String("Deleted Items".to_string()),
                ),
                (PID_TAG_CONTENT_COUNT, PcValue::I32(0)),
            ],
        )?;
        heap.finalize(hid)
    };
    layout.add_node_data(deleted_items_nid, deleted_items_heap, ipm_subtree_nid, 0)?;

    let deleted_items_hier_heap = {
        let mut heap = HeapBuilder::new(0xBC);
        let columns = [(PID_TAG_LTP_ROW_ID, PTYP_INTEGER_32, 0u16, 4u8, 0u8)];
        let hid = build_tc_inline_checked(&mut heap, &columns, &[])?;
        heap.finalize(hid)
    };
    layout.add_node_data(
        (deleted_items_nid & !0x1F) | 0x0D,
        deleted_items_hier_heap,
        0,
        0,
    )?;

    let deleted_items_cont_heap = {
        let mut heap = HeapBuilder::new(0xBC);
        let columns = [(PID_TAG_LTP_ROW_ID, PTYP_INTEGER_32, 0u16, 4u8, 0u8)];
        let hid = build_tc_inline_checked(&mut heap, &columns, &[])?;
        heap.finalize(hid)
    };
    layout.add_node_data(
        (deleted_items_nid & !0x1F) | 0x0E,
        deleted_items_cont_heap,
        0,
        0,
    )?;

    let deleted_items_assoc_cont_heap = {
        let mut heap = HeapBuilder::new(0xBC);
        let columns = [(PID_TAG_LTP_ROW_ID, PTYP_INTEGER_32, 0u16, 4u8, 0u8)];
        let hid = build_tc_inline_checked(&mut heap, &columns, &[])?;
        heap.finalize(hid)
    };
    layout.add_node_data(
        (deleted_items_nid & !0x1F) | 0x0F,
        deleted_items_assoc_cont_heap,
        0,
        0,
    )?;

    // ── Search Root folder (§4 of the round-9 verified MS-PST data) ─────────
    //
    // "The basic schema requirements of the search Folder object PC are
    // identical to the Folder object PC" (verified, round 9) — this is given
    // the same PC + hierarchy TC (empty) + contents TC (empty) + associated-
    // contents TC (empty) shape as the other folders here (the safer,
    // more-complete-looking interpretation over a bare PC-only guess). v1
    // never implements search-criteria semantics or search-execution logic
    // and never populates this with results — it is a minimal, valid,
    // always-empty container, referenced by `PidTagFinderEntryId` on the
    // message store PC below. NOT a child of IPM_SUBTREE's hierarchy TC (see
    // `search_root_nid`'s allocation comment above).
    let search_root_heap = {
        let mut heap = HeapBuilder::new(0x6C);
        let hid = build_pc_v2(
            &mut heap,
            &[
                (
                    PID_TAG_DISPLAY_NAME,
                    PcValue::String("Search Root".to_string()),
                ),
                (PID_TAG_CONTENT_COUNT, PcValue::I32(0)),
            ],
        )?;
        heap.finalize(hid)
    };
    layout.add_node_data(search_root_nid, search_root_heap, 0, 0)?;

    let search_root_hier_heap = {
        let mut heap = HeapBuilder::new(0xBC);
        let columns = [(PID_TAG_LTP_ROW_ID, PTYP_INTEGER_32, 0u16, 4u8, 0u8)];
        let hid = build_tc_inline_checked(&mut heap, &columns, &[])?;
        heap.finalize(hid)
    };
    layout.add_node_data(
        (search_root_nid & !0x1F) | 0x0D,
        search_root_hier_heap,
        0,
        0,
    )?;

    let search_root_cont_heap = {
        let mut heap = HeapBuilder::new(0xBC);
        let columns = [(PID_TAG_LTP_ROW_ID, PTYP_INTEGER_32, 0u16, 4u8, 0u8)];
        let hid = build_tc_inline_checked(&mut heap, &columns, &[])?;
        heap.finalize(hid)
    };
    layout.add_node_data(
        (search_root_nid & !0x1F) | 0x0E,
        search_root_cont_heap,
        0,
        0,
    )?;

    let search_root_assoc_cont_heap = {
        let mut heap = HeapBuilder::new(0xBC);
        let columns = [(PID_TAG_LTP_ROW_ID, PTYP_INTEGER_32, 0u16, 4u8, 0u8)];
        let hid = build_tc_inline_checked(&mut heap, &columns, &[])?;
        heap.finalize(hid)
    };
    layout.add_node_data(
        (search_root_nid & !0x1F) | 0x0F,
        search_root_assoc_cont_heap,
        0,
        0,
    )?;

    let folder_name = if opts.folder_display_name.is_empty() {
        "Unique Mail".to_string()
    } else {
        opts.folder_display_name.clone()
    };

    // `PidTagContainerClass` = "IPF.Note" (§3.2, review round 3 P2 — spec.md
    // §3.2's LOCKED table requires "standard display name / container class
    // for IPM subtree per MS-PST messaging conventions"). "Unique Mail" is
    // v1's single mail-containing folder under IPM_SUBTREE, holding
    // `IPM.Note`-class messages, so this is the folder MS-PST/MAPI convention
    // puts the container class on — not IPM_SUBTREE itself (see comment
    // above the IPM_SUBTREE PC build). See
    // `docs/pst-writer-fidelity-v1.md` for the full auditable reasoning.
    let unique_heap = {
        let mut heap = HeapBuilder::new(0x6C);
        let hid = build_pc_v2(
            &mut heap,
            &[
                (PID_TAG_DISPLAY_NAME, PcValue::String(folder_name)),
                (PID_TAG_CONTENT_COUNT, PcValue::I32(messages.len() as i32)),
                (
                    PID_TAG_CONTAINER_CLASS,
                    PcValue::String("IPF.Note".to_string()),
                ),
            ],
        )?;
        heap.finalize(hid)
    };
    layout.add_node_data(unique_mail_nid, unique_heap, ipm_subtree_nid, 0)?;

    let unique_hier_heap = {
        let mut heap = HeapBuilder::new(0xBC);
        let columns = [(PID_TAG_LTP_ROW_ID, PTYP_INTEGER_32, 0u16, 4u8, 0u8)];
        let hid = build_tc_inline_checked(&mut heap, &columns, &[])?;
        heap.finalize(hid)
    };
    layout.add_node_data((unique_mail_nid & !0x1F) | 0x0D, unique_hier_heap, 0, 0)?;

    // Associated-contents (FAI) table, empty — see the Root folder's comment
    // above for the MS-PST §2.4.2 rationale and NID-suffix cross-check.
    let unique_assoc_cont_heap = {
        let mut heap = HeapBuilder::new(0xBC);
        let columns = [(PID_TAG_LTP_ROW_ID, PTYP_INTEGER_32, 0u16, 4u8, 0u8)];
        let hid = build_tc_inline_checked(&mut heap, &columns, &[])?;
        heap.finalize(hid)
    };
    layout.add_node_data(
        (unique_mail_nid & !0x1F) | 0x0F,
        unique_assoc_cont_heap,
        0,
        0,
    )?;

    // ── Messages ──────────────────────────────────────────────────────────────
    let mut message_nids: Vec<u64> = Vec::with_capacity(messages.len());
    let mut messages_with_incomplete_body: u64 = 0;
    let mut messages_with_unavailable_body: u64 = 0;
    for msg in &messages {
        let nid = build_message_node(&mut layout, msg, unique_mail_nid)?;
        message_nids.push(nid);
        if msg.body_incomplete {
            messages_with_incomplete_body += 1;
        }
        if msg.body_unavailable {
            messages_with_unavailable_body += 1;
        }
    }
    let messages_written = message_nids.len() as u64;

    let unique_cont_heap = {
        let mut heap = HeapBuilder::new(0xBC);
        let columns = [(PID_TAG_LTP_ROW_ID, PTYP_INTEGER_32, 0u16, 4u8, 0u8)];
        let rows: Vec<Vec<u8>> = message_nids
            .iter()
            .map(|n| (*n as u32).to_le_bytes().to_vec())
            .collect();
        let hid = build_tc_inline_checked(&mut heap, &columns, &rows)?;
        heap.finalize(hid)
    };
    layout.add_node_data((unique_mail_nid & !0x1F) | 0x0E, unique_cont_heap, 0, 0)?;

    // ── Message store (PidTagIpmSubtreeEntryId — §3.2, review fold #2;
    // PidTagRecordKey — round-5 cross-model review finding, Part A;
    // PidTagIpmWastebasketEntryId / PidTagFinderEntryId — §1 of the round-9
    // verified MS-PST data, superseding the prior D-0068-05 decline) ────────
    //
    // The store's own `PidTagRecordKey` (0x0FF9) and each EntryID's 16-byte
    // ProviderUID must be the *same* value: a store-internal EntryID's
    // provider UID is conventionally the store's own unique record key, not
    // an arbitrary placeholder, so every EntryID genuinely identifies this
    // specific store. Generated once per write and reused in all three
    // EntryIDs plus the record key property itself.
    let record_key = generate_store_record_key(path, messages.len());
    let ipm_subtree_entry_id = build_folder_entry_id(ipm_subtree_nid, &record_key);
    let wastebasket_entry_id = build_folder_entry_id(deleted_items_nid, &record_key);
    let finder_entry_id = build_folder_entry_id(search_root_nid, &record_key);
    let store_heap = {
        let mut heap = HeapBuilder::new(0x6C);
        let hid = build_pc_v2(
            &mut heap,
            &[
                (
                    PID_TAG_DISPLAY_NAME,
                    PcValue::String("Personal Folders".to_string()),
                ),
                (
                    PID_TAG_IPM_SUBTREE_ENTRYID,
                    PcValue::Binary(ipm_subtree_entry_id),
                ),
                (
                    PID_TAG_IPM_WASTEBASKET_ENTRYID,
                    PcValue::Binary(wastebasket_entry_id),
                ),
                (PID_TAG_FINDER_ENTRYID, PcValue::Binary(finder_entry_id)),
                (PID_TAG_RECORD_KEY, PcValue::Binary(record_key.to_vec())),
            ],
        )?;
        heap.finalize(hid)
    };
    layout.add_node_data(NID_MESSAGE_STORE, store_heap, 0, 0)?;

    // ── Fixed MS-PST "template objects" (§5 of the round-9 verified MS-PST
    // data, superseding the prior round-6 template-objects decline note) ────
    //
    // Four fixed-NID, always-zero-row TCs: each MUST have no data rows
    // (verified on every one of the four source pages) — only the TCINFO
    // column-descriptor byte-width bookkeeping needs to be correct, not any
    // row content. Registered the same way as other top-level nodes with no
    // parent/subnode (`NID_MESSAGE_STORE`/`NID_NAME_TO_ID_MAP` above).
    let hierarchy_template_heap = {
        let mut heap = HeapBuilder::new(0xBC);
        let (columns, total_row_width) =
            build_template_tc_columns(&HIERARCHY_TABLE_TEMPLATE_COLUMNS);
        let hid = build_tc_inline_checked_sized(&mut heap, &columns, &[], total_row_width)?;
        heap.finalize(hid)
    };
    layout.add_node_data(NID_HIERARCHY_TABLE_TEMPLATE, hierarchy_template_heap, 0, 0)?;

    let contents_template_heap = {
        let mut heap = HeapBuilder::new(0xBC);
        let (columns, total_row_width) =
            build_template_tc_columns(&CONTENTS_TABLE_TEMPLATE_COLUMNS);
        let hid = build_tc_inline_checked_sized(&mut heap, &columns, &[], total_row_width)?;
        heap.finalize(hid)
    };
    layout.add_node_data(NID_CONTENTS_TABLE_TEMPLATE, contents_template_heap, 0, 0)?;

    let assoc_contents_template_heap = {
        let mut heap = HeapBuilder::new(0xBC);
        let (columns, total_row_width) =
            build_template_tc_columns(&ASSOC_CONTENTS_TABLE_TEMPLATE_COLUMNS);
        let hid = build_tc_inline_checked_sized(&mut heap, &columns, &[], total_row_width)?;
        heap.finalize(hid)
    };
    layout.add_node_data(
        NID_ASSOC_CONTENTS_TABLE_TEMPLATE,
        assoc_contents_template_heap,
        0,
        0,
    )?;

    let search_contents_template_heap = {
        let mut heap = HeapBuilder::new(0xBC);
        let (columns, total_row_width) =
            build_template_tc_columns(&SEARCH_CONTENTS_TABLE_TEMPLATE_COLUMNS);
        let hid = build_tc_inline_checked_sized(&mut heap, &columns, &[], total_row_width)?;
        heap.finalize(hid)
    };
    layout.add_node_data(
        NID_SEARCH_CONTENTS_TABLE_TEMPLATE,
        search_contents_template_heap,
        0,
        0,
    )?;

    // ── AMap + BTree pages, then real file offsets ───────────────────────────
    layout.reserve_page(PTYPE_AMAP);

    let nbt_plan = layout.plan_tree(PTYPE_NBT, NBT_LEAF_ENTRY_SIZE, layout.nodes.len());
    let bbt_plan = layout.plan_tree(PTYPE_BBT, BBT_LEAF_ENTRY_SIZE, layout.blocks.len());

    layout.calculate_offsets();

    // ── Write to a temp sibling, then atomically rename over `path` ─────────
    let tmp_path = temp_sibling_path(path);
    // Same protected-source enforcement as `path` above (§3.7 rule 2 / Core
    // Mandate #3), applied to the temp-staging path too — this is where
    // `File::create` below actually writes bytes first, so it must be
    // checked BEFORE that call, not just at the final rename target.
    check_not_protected_source(&tmp_path, protected_source_paths)?;
    {
        let mut file = File::create(&tmp_path)?;
        write_header_v1(&mut file, &layout, &nbt_plan, &bbt_plan)?;
        write_amap_page_v1(&mut file, &layout)?;

        let page_offsets = page_offset_map(&layout);
        write_nbt(&mut file, &layout, &nbt_plan, &page_offsets)?;
        write_bbt(&mut file, &layout, &bbt_plan, &page_offsets)?;

        for block in &layout.blocks {
            file.seek(SeekFrom::Start(block.offset))?;
            write_data_block(&mut file, block.bid, &block.data)?;
        }
        file.flush()?;
    }

    if let Err(e) = fs::rename(&tmp_path, path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(WriterError::Io(e));
    }

    Ok(WritePstReport {
        messages_written,
        messages_skipped: 0,
        path: path.to_path_buf(),
        bytes: layout.file_size(),
        messages_with_incomplete_body,
        messages_with_unavailable_body,
    })
}

/// Process-wide entropy suffix for temp-staging filenames (see
/// `temp_sibling_path`), computed lazily once per process and cached.
///
/// Follows this file's `generate_store_record_key` pattern rather than
/// adding a new crate dependency (`uuid`/`rand`/`tempfile`): a
/// `crc32fast::hash` over wall-clock nanoseconds since the epoch plus the
/// current process ID. It is deliberately cached per-process (not
/// recomputed on every call) so that repeated `temp_sibling_path` calls for
/// the same destination within one run — including a test calling it
/// directly to predict what `write_unicode_pst` will compute internally —
/// observe the identical value; a fresh process (a later run, a crashed-and-
/// restarted one, or an attacker who doesn't share this process's PID/start
/// time) gets a different one. This only needs to reduce the *ambient*
/// chance that a temp-staging name collides with an unrelated file (e.g. a
/// leftover artifact from a previous crashed run, or an adversarial/
/// mistaken input named to match the old purely-PID-based scheme) — it is
/// not the sole protection: `write_unicode_pst` also runs an explicit
/// `check_not_protected_source` against the computed temp path before ever
/// calling `File::create` on it.
fn process_entropy_suffix() -> &'static str {
    static SUFFIX: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    SUFFIX.get_or_init(|| {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let pid = std::process::id();

        let mut seed = Vec::with_capacity(24);
        seed.extend_from_slice(&nanos.to_le_bytes());
        seed.extend_from_slice(&pid.to_le_bytes());

        format!("{:08x}", crc32fast::hash(&seed))
    })
}

/// Compute the temp-staging sibling path `write_unicode_pst` writes the full
/// file to before atomically renaming over `path` on success (§3.7 rule 1).
///
/// The name is `<file_name>.tmp-<pid>-<entropy>`, where `<entropy>` is
/// [`process_entropy_suffix`] — an 8-hex-digit `crc32fast` hash over
/// wall-clock nanoseconds and the process ID, not just the PID alone. A
/// purely PID-based name (the v1 scheme this replaces) is a known anti-
/// pattern for temp-file naming: PIDs are reused across process lifetimes
/// and form a small, predictable space, so a stale file left by a crashed
/// prior run, or an adversarial/mistaken input, could plausibly share the
/// exact computed name. Adding the entropy suffix reduces that ambient
/// collision likelihood; it does not need to eliminate it, because
/// `write_unicode_pst` also runs an explicit `check_not_protected_source`
/// against the returned path before `File::create` ever touches it — this
/// function's job is defense in depth, not the sole guarantee.
///
/// `pub` (not `pub(crate)`) specifically so the `pst-writer` integration
/// test suite (`tests/writer_v1.rs`, a separate crate) can call it directly
/// to compute the *exact* temp path `write_unicode_pst` will use for a given
/// destination, rather than re-guessing the naming scheme in test code and
/// risking silent drift from the real implementation.
pub fn temp_sibling_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .map(|f| f.to_string_lossy().into_owned())
        .unwrap_or_else(|| "output.pst".to_string());
    let tmp_name = format!(
        "{file_name}.tmp-{}-{}",
        std::process::id(),
        process_entropy_suffix()
    );
    match path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir.join(tmp_name),
        _ => PathBuf::from(tmp_name),
    }
}

// ── Message node building (§3.3) ─────────────────────────────────────────────

fn next_subnode_nid(counter: &mut u32) -> u64 {
    *counter += 1;
    // Low 5 bits = 0x1F (LTP type marker) so `Hid::hid_type() != 0`, distinguishing
    // a subnode NID from a heap HID (whose low 5 bits are always 0 by construction).
    ((*counter as u64) << 5) | 0x1F
}

fn utf16le_bytes(s: &str) -> Vec<u8> {
    s.encode_utf16().flat_map(|c| c.to_le_bytes()).collect()
}

fn build_message_node(layout: &mut Layout, msg: &WriteMessage, parent_nid: u64) -> Result<u64> {
    let msg_nid = layout.alloc_nid(NID_TYPE_NORMAL_MESSAGE);

    let plain_text: Option<&str> = if msg.body_unavailable {
        None
    } else {
        msg.body_plain.as_deref()
    };
    let html_bytes: Option<&[u8]> = if msg.body_unavailable {
        None
    } else {
        msg.body_html.as_deref().filter(|b| !b.is_empty())
    };

    let mut subnode_entries: Vec<(u64, u64, u64)> = Vec::new();
    let mut subnode_counter = 0u32;
    let mut written_content_bytes: u64 = 0;

    let mut props: Vec<(u16, PcValue)> = Vec::new();
    if let Some(mid) = &msg.message_id {
        props.push((PID_TAG_INTERNET_MESSAGE_ID, PcValue::String(mid.clone())));
    }
    props.push((PID_TAG_SUBJECT, PcValue::String(msg.subject.clone())));
    if let Some(sender) = &msg.sender {
        props.push((
            PID_TAG_SENDER_EMAIL_ADDRESS,
            PcValue::String(sender.clone()),
        ));
    }
    if let Some(display_to) = &msg.display_to {
        props.push((PID_TAG_DISPLAY_TO, PcValue::String(display_to.clone())));
    }
    if let Some(submit_time) = msg.submit_time {
        props.push((PID_TAG_CLIENT_SUBMIT_TIME, PcValue::Time(submit_time)));
    }
    // PidTagMessageFlags (0x0E07): a sane constant default — MSGFLAG_READ
    // (0x00000001) — reasonable for an exported "unique mail" archive item.
    // Not an unread-tracking feature; just a fixed value every message gets.
    props.push((PID_TAG_MESSAGE_FLAGS, PcValue::I32(0x0000_0001)));
    // PidTagCreationTime / PidTagLastModificationTime: this is a
    // synthetically-written export item, not a live mailbox object, so
    // `submit_time` (when present) is a defensible stand-in for both when no
    // better source exists. Never invented — omitted entirely when
    // `submit_time` is `None` (consistent with this track's body_unavailable
    // "never invent" principle).
    if let Some(submit_time) = msg.submit_time {
        props.push((PID_TAG_CREATION_TIME, PcValue::Time(submit_time)));
        props.push((PID_TAG_LAST_MODIFICATION_TIME, PcValue::Time(submit_time)));
    }
    let message_class = msg
        .message_class
        .clone()
        .unwrap_or_else(|| "IPM.Note".to_string());
    props.push((PID_TAG_MESSAGE_CLASS, PcValue::String(message_class)));
    // Attachments dropped in v1 (0069) — never claim attach fidelity.
    props.push((PID_TAG_HAS_ATTACHMENTS, PcValue::Bool(false)));

    if let Some(plain) = plain_text {
        let bytes = utf16le_bytes(plain);
        if bytes.len() > MAX_HEAP_VALUE_SIZE {
            // Diverted to a subnode: the PC only holds a small SubnodeString
            // reference, so these raw bytes are NOT captured by `probe_bytes`
            // below and must be added here.
            written_content_bytes += bytes.len() as u64;
            let sub_nid = next_subnode_nid(&mut subnode_counter);
            let bid_data = layout.write_data_chain(bytes)?;
            subnode_entries.push((sub_nid, bid_data, 0));
            props.push((PID_TAG_BODY, PcValue::SubnodeString(sub_nid)));
        } else {
            // Written inline as a PC heap value: already counted by
            // `probe_bytes` below, so must NOT be added here too (would
            // double-count).
            props.push((PID_TAG_BODY, PcValue::String(plain.to_string())));
        }
    }
    if let Some(html) = html_bytes {
        if html.len() > MAX_HEAP_VALUE_SIZE {
            // Diverted to a subnode — see plain-body comment above.
            written_content_bytes += html.len() as u64;
            let sub_nid = next_subnode_nid(&mut subnode_counter);
            let bid_data = layout.write_data_chain(html.to_vec())?;
            subnode_entries.push((sub_nid, bid_data, 0));
            props.push((PID_TAG_BODY_HTML, PcValue::SubnodeBinary(sub_nid)));
        } else {
            // Written inline — already counted by `probe_bytes` below.
            props.push((PID_TAG_BODY_HTML, PcValue::Binary(html.to_vec())));
        }
    }

    // Native body / editor format / codepage (§3.3.1) — match what was actually
    // written; v1 never writes RTF, so there is nothing RTF-related to clear.
    if html_bytes.is_some() {
        props.push((PID_TAG_NATIVE_BODY, PcValue::I32(3))); // HTML
        props.push((PID_TAG_MESSAGE_EDITOR_FORMAT, PcValue::I32(2)));
        props.push((PID_TAG_INTERNET_CODEPAGE, PcValue::I32(65001)));
    } else if plain_text.is_some() {
        props.push((PID_TAG_NATIVE_BODY, PcValue::I32(1))); // Plain
        props.push((PID_TAG_MESSAGE_EDITOR_FORMAT, PcValue::I32(1)));
        props.push((PID_TAG_INTERNET_CODEPAGE, PcValue::I32(65001)));
    }
    // No body written at all: omit NativeBody/EditorFormat/Codepage entirely.

    // PidTagMessageSize (§3.3.2) — computed from bytes actually written, never
    // copied from a source-declared size. Probe the PC heap size *without* the
    // MessageSize property itself (self-referential), then add subnode-diverted
    // body/html raw byte lengths.
    let mut probe_heap = HeapBuilder::new(0x6C);
    let probe_hid = build_pc_v2(&mut probe_heap, &props)?;
    let probe_bytes = probe_heap.finalize(probe_hid);
    let message_size_u64 = probe_bytes.len() as u64 + written_content_bytes;
    let message_size = i32::try_from(message_size_u64).map_err(|_| {
        WriterError::BodyTooLarge(format!(
            "computed message size {message_size_u64} bytes exceeds \
             PidTagMessageSize's PT_LONG (MS-OXPROPS) range ({} bytes max) — \
             refusing to silently clamp a size that would misrepresent what \
             was written",
            i32::MAX
        ))
    })?;
    props.push((PID_TAG_MESSAGE_SIZE, PcValue::I32(message_size)));

    let mut heap = HeapBuilder::new(0x6C);
    let hid = build_pc_v2(&mut heap, &props)?;
    let msg_heap_bytes = heap.finalize(hid);

    let sub_bid = if subnode_entries.is_empty() {
        0
    } else {
        layout.add_subnode_leaf(&subnode_entries)?
    };

    layout.add_node_data(msg_nid, msg_heap_bytes, parent_nid, sub_bid)?;
    Ok(msg_nid)
}

/// PST-local Folder EntryID, used for `PidTagIpmSubtreeEntryId` (§3.2),
/// `PidTagIpmWastebasketEntryId`, and `PidTagFinderEntryId` (§1 of the
/// round-9 verified MS-PST data) alike — generalized from the
/// IPM_SUBTREE-only `build_ipm_subtree_entry_id` (its original name) once a
/// second and third caller (Deleted Items, Search Root) needed the identical
/// shape for a different target folder NID.
///
/// Design decision: `pst-reader` does not parse/resolve EntryIDs at all (it
/// walks folders by NID directly), and Outlook/scanpst are not available in
/// this environment to independently verify EntryID acceptance — this is
/// therefore a documented, best-effort MS-OXCDATA-shaped structure, not one
/// verified against a real Outlook-opened PST:
///
/// `abFlags(4) = 0` + `ProviderUID(16)` (matches the store's own
/// `PidTagRecordKey`, `provider_uid`, byte-for-byte — a store-internal
/// EntryID's provider UID is conventionally the store's own unique record
/// key, so the EntryID genuinely identifies this specific store rather than
/// carrying an arbitrary value) + `folder_nid` encoded as a 4-byte LE value
/// (its "internal reference"). Total 24 bytes. Still not independently
/// verified against a real Outlook-opened PST in this environment — flagged
/// as a residual for operator scanpst/Outlook verification per spec
/// §3.9-7/8 — see final report.
fn build_folder_entry_id(folder_nid: u64, provider_uid: &[u8; 16]) -> Vec<u8> {
    let mut id = Vec::with_capacity(24);
    id.extend_from_slice(&0u32.to_le_bytes());
    id.extend_from_slice(provider_uid);
    id.extend_from_slice(&(folder_nid as u32).to_le_bytes());
    id
}

/// Best-effort unique 16-byte "store record key" for this write, used as both
/// the store's `PidTagRecordKey` (0x0FF9) and the EntryID's ProviderUID (see
/// `build_folder_entry_id`) so the two are self-consistent.
///
/// This is **not** a cryptographic GUID and makes no uniqueness guarantee
/// beyond "reasonably unlikely to collide across separate writer
/// invocations". Per this crate's minimal-dependency convention (see
/// `.agents/skills/coding-core/SKILL.md`: "Keep dependencies permissive and
/// minimal" — `pst-writer` already depends on `crc32fast`, `chrono`, and
/// `byteorder` and adds no new crate for this), it is derived from
/// write-time-varying inputs already available without pulling in `uuid` or
/// `rand`: wall-clock nanoseconds since the epoch, the current process ID,
/// and the destination path together with the message count (something
/// write-specific). Each of four differently-salted `crc32fast::hash` calls
/// over that input produces one `u32`; concatenated they form 16 bytes. This
/// only needs to be non-zero, self-consistent (used identically in both
/// places it's written), and reasonably unique per invocation — it is
/// explicitly not a substitute for a real GUID/UUID.
fn generate_store_record_key(path: &Path, message_count: usize) -> [u8; 16] {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let pid = std::process::id();
    let path_bytes = path.to_string_lossy().into_owned().into_bytes();

    let mut seed_input = Vec::with_capacity(path_bytes.len() + 32);
    seed_input.extend_from_slice(&nanos.to_le_bytes());
    seed_input.extend_from_slice(&pid.to_le_bytes());
    seed_input.extend_from_slice(&(message_count as u64).to_le_bytes());
    seed_input.extend_from_slice(&path_bytes);

    let mut key = [0u8; 16];
    let salts: [u32; 4] = [0x5A17_0001, 0x5A17_0002, 0x5A17_0003, 0x5A17_0004];
    for (i, salt) in salts.into_iter().enumerate() {
        let mut salted = Vec::with_capacity(seed_input.len() + 4);
        salted.extend_from_slice(&salt.to_le_bytes());
        salted.extend_from_slice(&seed_input);
        let hash = crc32fast::hash(&salted);
        key[i * 4..i * 4 + 4].copy_from_slice(&hash.to_le_bytes());
    }
    key
}

// ── PC value encoding (Result-based; no unwrap/expect/assert) ───────────────

/// Property value for the production PC builder. Distinct from `crate::PropertyValue`
/// (used by the fixture path only) — adds `Binary` and subnode-reference variants.
#[derive(Debug, Clone)]
pub enum PcValue {
    I32(i32),
    Bool(bool),
    Time(i64),
    String(String),
    Binary(Vec<u8>),
    /// Value already stored in a subnode (see module docs); stores the raw NID
    /// as `dwValueHnid` with `PtypString`.
    SubnodeString(u64),
    /// As `SubnodeString`, with `PtypBinary`.
    SubnodeBinary(u64),
}

fn encode_pc_value(heap: &mut HeapBuilder, value: &PcValue) -> Result<Vec<u8>> {
    let mut r = Vec::with_capacity(6);
    match value {
        PcValue::I32(v) => {
            r.extend_from_slice(&PTYP_INTEGER_32.to_le_bytes());
            r.extend_from_slice(&v.to_le_bytes());
        }
        PcValue::Bool(v) => {
            r.extend_from_slice(&PTYP_BOOLEAN.to_le_bytes());
            r.extend_from_slice(&(*v as u32).to_le_bytes());
        }
        PcValue::Time(v) => {
            r.extend_from_slice(&PTYP_TIME.to_le_bytes());
            let hid = heap.try_alloc(&v.to_le_bytes())?;
            r.extend_from_slice(&hid.to_le_bytes());
        }
        PcValue::String(s) => {
            r.extend_from_slice(&PTYP_STRING.to_le_bytes());
            let utf16 = utf16le_bytes(s);
            let hid = heap.try_alloc(&utf16)?;
            r.extend_from_slice(&hid.to_le_bytes());
        }
        PcValue::Binary(b) => {
            r.extend_from_slice(&PTYP_BINARY.to_le_bytes());
            let hid = heap.try_alloc(b)?;
            r.extend_from_slice(&hid.to_le_bytes());
        }
        PcValue::SubnodeString(nid) => {
            r.extend_from_slice(&PTYP_STRING.to_le_bytes());
            r.extend_from_slice(&(*nid as u32).to_le_bytes());
        }
        PcValue::SubnodeBinary(nid) => {
            r.extend_from_slice(&PTYP_BINARY.to_le_bytes());
            r.extend_from_slice(&(*nid as u32).to_le_bytes());
        }
    }
    Ok(r)
}

/// Build a Property Context (Result-based; production path).
pub fn build_pc_v2(heap: &mut HeapBuilder, properties: &[(u16, PcValue)]) -> Result<u32> {
    let mut records: Vec<(u16, Vec<u8>)> = Vec::with_capacity(properties.len());
    for (prop_id, value) in properties {
        records.push((*prop_id, encode_pc_value(heap, value)?));
    }
    build_bth_checked(heap, 2, 6, &mut records)
}

/// Result-based BTH builder mirroring `crate::build_bth` (fixture path keeps the
/// original, panic-free-but-unchecked version for its own use).
pub fn build_bth_checked(
    heap: &mut HeapBuilder,
    cb_key: u8,
    cb_ent: u8,
    records: &mut [(u16, Vec<u8>)],
) -> Result<u32> {
    records.sort_by_key(|r| r.0);

    let mut bth_data = vec![0xB5, cb_key, cb_ent, 0];
    bth_data.extend_from_slice(&0u32.to_le_bytes());
    let hid_root = heap.try_alloc(&bth_data)?;

    let mut leaf_data = Vec::new();
    for (key, data) in records.iter() {
        leaf_data.extend_from_slice(&key.to_le_bytes());
        leaf_data.extend_from_slice(data);
    }
    let hid_leaf = heap.try_alloc(&leaf_data)?;

    heap.patch_u32(hid_root, 4, hid_leaf)?;
    Ok(hid_root)
}

/// Result-based inline TC builder mirroring `crate::build_tc_inline`.
///
/// `rgib[3]` (the total row width, used by `pst_reader::ltp::tc` as the row
/// stride when dividing up row data) is derived from `rows.first()`'s actual
/// length — correct for every call site in this file that passes real row
/// data, but degenerates to `0` for a table that is defined to always have
/// zero rows (there is no row to measure). The four fixed MS-PST "template
/// object" tables (§5 of the round-9 verified data) are always zero-row by
/// spec, yet still need a correct, non-degenerate row width in their TCINFO
/// header for a reader to parse the column schema without error — see
/// [`build_tc_inline_checked_sized`], which this function now delegates to.
pub fn build_tc_inline_checked(
    heap: &mut HeapBuilder,
    columns: &[(u16, u16, u16, u8, u8)],
    rows: &[Vec<u8>],
) -> Result<u32> {
    let total_row_width = rows.first().map(|r| r.len() as u16).unwrap_or(0);
    build_tc_inline_checked_sized(heap, columns, rows, total_row_width)
}

/// As [`build_tc_inline_checked`], but with an explicit `total_row_width`
/// (`TCINFO.rgib[3]`) instead of inferring it from `rows.first()`. Needed for
/// the four fixed MS-PST template-object tables (§5 of the round-9 verified
/// data), which are always zero-row (`rows` is always `&[]`) but must still
/// carry a correct row-width value derived from their real column schema —
/// see [`build_template_tc_columns`], which computes both the column
/// descriptors and this width together so they can never drift apart.
pub fn build_tc_inline_checked_sized(
    heap: &mut HeapBuilder,
    columns: &[(u16, u16, u16, u8, u8)],
    rows: &[Vec<u8>],
    total_row_width: u16,
) -> Result<u32> {
    let mut tcinfo = Vec::new();
    tcinfo.push(0x7C);
    tcinfo.push(columns.len() as u8);

    tcinfo.extend_from_slice(&0u16.to_le_bytes());
    tcinfo.extend_from_slice(&0u16.to_le_bytes());
    tcinfo.extend_from_slice(&0u16.to_le_bytes());
    tcinfo.extend_from_slice(&total_row_width.to_le_bytes());
    tcinfo.extend_from_slice(&0u32.to_le_bytes()); // hidRowIndex (none — v1 tables are tiny)
    tcinfo.extend_from_slice(&0u32.to_le_bytes()); // hnidRows placeholder, patched below

    for col in columns {
        tcinfo.extend_from_slice(&col.0.to_le_bytes());
        tcinfo.extend_from_slice(&col.1.to_le_bytes());
        tcinfo.extend_from_slice(&col.2.to_le_bytes());
        tcinfo.push(col.3);
        tcinfo.push(col.4);
    }

    let hid_tcinfo = heap.try_alloc(&tcinfo)?;

    let mut row_data = Vec::new();
    for row in rows {
        row_data.extend_from_slice(row);
    }
    let hid_rows = heap.try_alloc(&row_data)?;

    heap.patch_u32(hid_tcinfo, 14, hid_rows)?;
    Ok(hid_tcinfo)
}

// ── Fixed MS-PST "template object" column schemas (§5 of the round-9
// verified MS-PST data) ──────────────────────────────────────────────────

/// A TCOLDESC tuple: `(prop_id, prop_type, ib_data, cb_data, i_bit)`. Named
/// alias for the 5-tuple already used positionally throughout this file
/// (`build_tc_inline_checked` and friends) — introduced alongside
/// [`build_template_tc_columns`] so its `Vec<...>` return type stays a single
/// named type rather than a directly-nested 5-tuple (clippy
/// `type_complexity`).
type TcColumnTuple = (u16, u16, u16, u8, u8);

/// A TC column's storage width class, used only to compute correct
/// `ib_data`/`cb_data` byte-offset bookkeeping for the four always-empty
/// template tables below — this is deliberately a narrower, purpose-built
/// enum, not a general MAPI-type abstraction used elsewhere in this file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TcColType {
    /// PtypInteger32 (0x0003) — 4 bytes inline.
    I32,
    /// PtypInteger64 (0x0014) — 8 bytes inline.
    I64,
    /// PtypBoolean (0x000B) — 1 byte inline.
    Bool,
    /// PtypTime (0x0040) — 8 bytes inline (FILETIME).
    Time,
    /// PtypString (0x001F) — variable-length; the row stores a 4-byte HNID
    /// reference (matches `PID_TAG_LTP_ROW_ID`'s existing inline-I32
    /// precedent for *width*, but this is the HNID-reference case that
    /// `pst_reader::ltp::tc::TableContext::get_row_string` expects: `cb_data
    /// == 4`, `prop_type == 0x001F`).
    StringRef,
    /// PtypBinary (0x0102) — variable-length; 4-byte HNID reference, same
    /// convention as `StringRef`.
    BinaryRef,
    /// PtypMultipleInteger32 (0x1003) — variable-length MAPI multi-value
    /// type. This repo's TC column model has no existing precedent for a
    /// genuine `PtypMultiple*` value; per the verified source data's own
    /// guidance this is conservatively modeled as a 4-byte HNID reference,
    /// identical in *width* to `StringRef`/`BinaryRef` — documented judgment
    /// call, see final report. Never exercised beyond column-width
    /// bookkeeping since this table always has zero rows.
    MultiI32Ref,
}

impl TcColType {
    /// Row-storage width in bytes (MS-PST §2.3.4.1 TCINFO row-layout
    /// convention: fixed-size types store their value inline at this width;
    /// variable-length types store a 4-byte HNID reference at this width).
    fn width(self) -> u8 {
        match self {
            TcColType::I64 | TcColType::Time => 8,
            TcColType::I32
            | TcColType::StringRef
            | TcColType::BinaryRef
            | TcColType::MultiI32Ref => 4,
            TcColType::Bool => 1,
        }
    }

    /// The `wPropType` (TCOLDESC `prop_type`) written for this column — the
    /// *real* MAPI type (e.g. `0x001F` for a string), even when its row
    /// storage is an HNID reference rather than the value itself.
    fn prop_type(self) -> u16 {
        match self {
            TcColType::I32 => PTYP_INTEGER_32,
            TcColType::I64 => PTYP_INTEGER_64,
            TcColType::Bool => PTYP_BOOLEAN,
            TcColType::Time => PTYP_TIME,
            TcColType::StringRef => PTYP_STRING,
            TcColType::BinaryRef => PTYP_BINARY,
            TcColType::MultiI32Ref => PTYP_MULTIPLE_INTEGER_32,
        }
    }
}

/// Build TCOLDESC column tuples `(prop_id, prop_type, ib_data, cb_data,
/// i_bit)` for a fixed-NID template table, plus the resulting total row
/// width (`TCINFO.rgib[3]`) — computed together so the two can never drift
/// apart (see [`build_tc_inline_checked_sized`]).
///
/// Groups columns widest-first (8-byte, then 4-byte, then 1-byte — none of
/// the four verified template schemas need a 2-byte group) per MS-PST
/// §2.3.4.1's TCINFO row-layout convention, computing running `ib_data`
/// offsets within each group, then appends the existence-bitmap tail
/// (`ceil(cCols/8)` bytes, MS-PST §2.3.4.1) to get the total row width. Every
/// column gets a real TCOLDESC even though these tables are always empty
/// (zero data rows) — the byte-width bookkeeping must still be correct for a
/// reader to parse the TCINFO header without error (the explicit reason this
/// helper exists, rather than reusing the existing single-column
/// `(PID_TAG_LTP_ROW_ID, PTYP_INTEGER_32, 0, 4, 0)` pattern used for the
/// per-folder hierarchy/contents/assoc-contents tables elsewhere in this
/// file, none of which has more than one column).
fn build_template_tc_columns(specs: &[(u16, TcColType)]) -> (Vec<TcColumnTuple>, u16) {
    let mut group8: Vec<&(u16, TcColType)> = Vec::new();
    let mut group4: Vec<&(u16, TcColType)> = Vec::new();
    let mut group1: Vec<&(u16, TcColType)> = Vec::new();
    for spec in specs {
        match spec.1.width() {
            8 => group8.push(spec),
            4 => group4.push(spec),
            _ => group1.push(spec),
        }
    }

    let mut columns = Vec::with_capacity(specs.len());
    let mut offset: u16 = 0;
    for (idx, (tag, ty)) in group8.into_iter().chain(group4).chain(group1).enumerate() {
        let width = ty.width();
        let i_bit = idx as u8;
        columns.push((*tag, ty.prop_type(), offset, width, i_bit));
        offset += width as u16;
    }

    let bitmap_bytes = (specs.len() as u16).div_ceil(8);
    let total_row_width = offset + bitmap_bytes;
    (columns, total_row_width)
}

/// 5a. Hierarchy Table Template (NID `0x60D`) column schema — verified from
/// https://learn.microsoft.com/en-us/openspecs/office_file_formats/ms-pst/c08fb6cb-2d91-42e5-b70d-f3e4f9781a2a
const HIERARCHY_TABLE_TEMPLATE_COLUMNS: [(u16, TcColType); 13] = [
    (0x0E30, TcColType::I32),
    (0x0E33, TcColType::I64),
    (0x0E34, TcColType::BinaryRef),
    (0x0E38, TcColType::I32),
    (0x3001, TcColType::StringRef),
    (0x3602, TcColType::I32),
    (0x3603, TcColType::I32),
    (0x360A, TcColType::Bool),
    (0x3613, TcColType::BinaryRef),
    (0x6635, TcColType::I32),
    (0x6636, TcColType::I32),
    (0x67F2, TcColType::I32),
    (0x67F3, TcColType::I32),
];

/// 5b. Contents Table Template (NID `0x60E`) column schema — verified from
/// https://learn.microsoft.com/en-us/openspecs/office_file_formats/ms-pst/f58e1ea9-b592-408d-b89e-53fd4cd6024b
const CONTENTS_TABLE_TEMPLATE_COLUMNS: [(u16, TcColType); 27] = [
    (0x0017, TcColType::I32),
    (0x001A, TcColType::StringRef),
    (0x0036, TcColType::I32),
    (0x0037, TcColType::StringRef),
    (0x0039, TcColType::Time),
    (0x0042, TcColType::StringRef),
    (0x0057, TcColType::Bool),
    (0x0058, TcColType::Bool),
    (0x0070, TcColType::StringRef),
    (0x0071, TcColType::BinaryRef),
    (0x0E03, TcColType::StringRef),
    (0x0E04, TcColType::StringRef),
    (0x0E06, TcColType::Time),
    (0x0E07, TcColType::I32),
    (0x0E08, TcColType::I32),
    (0x0E17, TcColType::I32),
    (0x0E30, TcColType::I32),
    (0x0E33, TcColType::I64),
    (0x0E34, TcColType::BinaryRef),
    (0x0E38, TcColType::I32),
    (0x0E3C, TcColType::BinaryRef),
    (0x0E3D, TcColType::BinaryRef),
    (0x1097, TcColType::I32),
    (0x3008, TcColType::Time),
    (0x65C6, TcColType::I32),
    (0x67F2, TcColType::I32),
    (0x67F3, TcColType::I32),
];

/// 5c. FAI Contents Table Template (NID `0x60F`) column schema — verified
/// from https://learn.microsoft.com/en-us/openspecs/office_file_formats/ms-pst/b2e619a0-6a9c-4101-9dcb-340ac41cf308
const ASSOC_CONTENTS_TABLE_TEMPLATE_COLUMNS: [(u16, TcColType); 14] = [
    (0x001A, TcColType::StringRef),
    (0x0E07, TcColType::I32),
    (0x0E17, TcColType::I32),
    (0x3001, TcColType::StringRef),
    (0x67F2, TcColType::I32),
    (0x67F3, TcColType::I32),
    (0x6800, TcColType::StringRef),
    (0x6803, TcColType::Bool),
    (0x6805, TcColType::MultiI32Ref),
    (0x7003, TcColType::I32),
    (0x7004, TcColType::BinaryRef),
    (0x7005, TcColType::BinaryRef),
    (0x7006, TcColType::StringRef),
    (0x7007, TcColType::I32),
];

/// 5d. Search Folder Contents Table Template (NID `0x610`) column schema —
/// verified from https://learn.microsoft.com/en-us/openspecs/office_file_formats/ms-pst/cdcf9571-049f-47f5-b075-8374057134ec
/// (`0x0E07`/`0x0E17` appear twice in Microsoft's own published table; kept
/// once each here — a TC cannot have a duplicate column tag, so this is
/// treated as a documentation quirk on Microsoft's page, not replicated).
const SEARCH_CONTENTS_TABLE_TEMPLATE_COLUMNS: [(u16, TcColType); 18] = [
    (0x0017, TcColType::I32),
    (0x001A, TcColType::StringRef),
    (0x0036, TcColType::I32),
    (0x0E07, TcColType::I32),
    (0x0E17, TcColType::I32),
    (0x0037, TcColType::StringRef),
    (0x0042, TcColType::StringRef),
    (0x0057, TcColType::Bool),
    (0x0E03, TcColType::StringRef),
    (0x0E04, TcColType::StringRef),
    (0x0E05, TcColType::StringRef),
    (0x0E06, TcColType::Time),
    (0x0E08, TcColType::I32),
    (0x0E2A, TcColType::Bool),
    (0x3008, TcColType::Time),
    (0x67F1, TcColType::I32),
    (0x67F2, TcColType::I32),
    (0x67F3, TcColType::I32),
];

// ── Layout extensions: XBLOCK/XXBLOCK, subnodes, BTree planning ─────────────

/// Planned page BIDs for a full multi-level BTree, bottom-up
/// (`levels[0]` = leaf pages; `levels.last()` has exactly one page, the root).
struct TreePlan {
    ptype: u8,
    levels: Vec<Vec<u64>>,
    leaf_entry_size: usize,
    per_leaf_capacity: usize,
}

impl Layout {
    /// Write `data` as a single external block, an XBLOCK chain, or an XXBLOCK
    /// chain (MS-PST §2.2.2.8.3), returning the BID to use as a node's
    /// `bidData`. Returns the null BID (0) for empty data. Hard-fails (never
    /// silently truncates) when `data` exceeds documented XBLOCK/XXBLOCK
    /// capacity.
    pub fn write_data_chain(&mut self, data: Vec<u8>) -> Result<u64> {
        if data.is_empty() {
            return Ok(0);
        }
        if data.len() > i32::MAX as usize {
            return Err(WriterError::BodyTooLarge(format!(
                "{} bytes exceeds i32::MAX ({} bytes) — the largest value \
                 representable by PidTagMessageSize's PT_LONG (MS-OXPROPS) \
                 range, which every written body/html value must fit within",
                data.len(),
                i32::MAX
            )));
        }
        if data.len() <= MAX_BLOCK_DATA {
            let bid = self.alloc_bid(false);
            self.blocks.push(BlockEntry {
                bid,
                data,
                offset: 0,
            });
            return Ok(bid);
        }

        let total_len = data.len() as u32;
        let data_chunks: Vec<(u64, u32)> = data
            .chunks(MAX_BLOCK_DATA)
            .map(|c| {
                let bid = self.alloc_bid(false);
                let len = c.len() as u32;
                self.blocks.push(BlockEntry {
                    bid,
                    data: c.to_vec(),
                    offset: 0,
                });
                (bid, len)
            })
            .collect();

        if data_chunks.len() <= MAX_XBLOCK_ENTRIES {
            return self.build_xblock(&data_chunks);
        }

        let mut xblock_bids = Vec::new();
        for group in data_chunks.chunks(MAX_XBLOCK_ENTRIES) {
            xblock_bids.push(self.build_xblock(group)?);
        }
        if xblock_bids.len() > MAX_XBLOCK_ENTRIES {
            let max_bytes =
                (MAX_XBLOCK_ENTRIES as u64) * (MAX_XBLOCK_ENTRIES as u64) * (MAX_BLOCK_DATA as u64);
            return Err(WriterError::AllocationFailed(format!(
                "data requires {} XBLOCKs, exceeding one XXBLOCK's capacity of {} entries \
                 (v1's two-level XBLOCK/XXBLOCK scheme represents at most ~{max_bytes} bytes per value)",
                xblock_bids.len(),
                MAX_XBLOCK_ENTRIES
            )));
        }
        self.build_xxblock(&xblock_bids, total_len)
    }

    fn build_xblock(&mut self, chunks: &[(u64, u32)]) -> Result<u64> {
        let c_entries = chunks.len() as u16;
        let lcb_total: u32 = chunks.iter().map(|(_, l)| *l).sum();
        let mut payload = Vec::with_capacity(8 + chunks.len() * 8);
        payload.push(0x01); // btype
        payload.push(0x01); // cLevel = 1 (XBLOCK: children are data blocks)
        payload.extend_from_slice(&c_entries.to_le_bytes());
        payload.extend_from_slice(&lcb_total.to_le_bytes());
        for (bid, _) in chunks {
            payload.extend_from_slice(&bid.to_le_bytes());
        }
        let bid = self.alloc_bid(true);
        self.blocks.push(BlockEntry {
            bid,
            data: payload,
            offset: 0,
        });
        Ok(bid)
    }

    fn build_xxblock(&mut self, xblock_bids: &[u64], total_len: u32) -> Result<u64> {
        let mut payload = Vec::with_capacity(8 + xblock_bids.len() * 8);
        payload.push(0x01); // btype
        payload.push(0x02); // cLevel = 2 (XXBLOCK: children are XBLOCKs)
        payload.extend_from_slice(&(xblock_bids.len() as u16).to_le_bytes());
        payload.extend_from_slice(&total_len.to_le_bytes());
        for bid in xblock_bids {
            payload.extend_from_slice(&bid.to_le_bytes());
        }
        let bid = self.alloc_bid(true);
        self.blocks.push(BlockEntry {
            bid,
            data: payload,
            offset: 0,
        });
        Ok(bid)
    }

    /// Add a top-level node whose data may exceed one block (via
    /// `write_data_chain`). `sub_bid` is the node's subnode-BTree root BID (0 if
    /// none). Never reachable is the fixture path's `assert!`-based `add_node`.
    pub fn add_node_data(
        &mut self,
        nid: u64,
        data: Vec<u8>,
        nid_parent: u64,
        sub_bid: u64,
    ) -> Result<u64> {
        let bid_data = self.write_data_chain(data)?;
        self.nodes.push(NodeEntry {
            nid,
            bid_data,
            bid_sub: sub_bid,
            nid_parent,
        });
        Ok(bid_data)
    }

    /// Build a single-block SLBLOCK subnode leaf listing `entries` (nid,
    /// bidData, bidSub). v1 scope: at most a couple of large-value diversions
    /// per message (no attachments), so one SLBLOCK always suffices — returns a
    /// typed error rather than silently dropping entries if that ever changes.
    pub fn add_subnode_leaf(&mut self, entries: &[(u64, u64, u64)]) -> Result<u64> {
        let mut payload = Vec::with_capacity(8 + entries.len() * 24);
        payload.push(0x02); // btype (subnode block)
        payload.push(0x00); // cLevel = 0 (SLBLOCK: leaf)
        payload.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        payload.extend_from_slice(&0u32.to_le_bytes()); // reserved
        for (nid, bid_data, bid_sub) in entries {
            payload.extend_from_slice(&nid.to_le_bytes());
            payload.extend_from_slice(&bid_data.to_le_bytes());
            payload.extend_from_slice(&bid_sub.to_le_bytes());
        }
        if payload.len() > MAX_BLOCK_DATA {
            return Err(WriterError::Layout(format!(
                "{} subnode entries exceed v1's single-SLBLOCK capacity",
                entries.len()
            )));
        }
        let bid = self.alloc_bid(true);
        self.blocks.push(BlockEntry {
            bid,
            data: payload,
            offset: 0,
        });
        Ok(bid)
    }

    /// Reserve pages for a full multi-level BTree over `entry_count` leaf
    /// entries. Content is filled in later (see `write_nbt`/`write_bbt`) once
    /// real file offsets are known.
    fn plan_tree(&mut self, ptype: u8, leaf_entry_size: usize, entry_count: usize) -> TreePlan {
        let per_leaf = (488usize / leaf_entry_size).max(1);
        let leaf_count = entry_count.div_ceil(per_leaf).max(1);
        let mut levels: Vec<Vec<u64>> =
            vec![(0..leaf_count).map(|_| self.reserve_page(ptype)).collect()];

        while levels.last().map(|l| l.len()).unwrap_or(0) > 1 {
            let prev_len = levels.last().map(|l| l.len()).unwrap_or(0);
            let next_count = prev_len.div_ceil(INTERMEDIATE_ENTRIES_PER_PAGE).max(1);
            let next: Vec<u64> = (0..next_count).map(|_| self.reserve_page(ptype)).collect();
            levels.push(next);
        }

        TreePlan {
            ptype,
            levels,
            leaf_entry_size,
            per_leaf_capacity: per_leaf,
        }
    }
}

// ── CRC / wSig ────────────────────────────────────────────────────────────

/// MS-PST §2.2.2.7.1 page signature. `pst-reader` does not validate this value
/// (see `pst_reader::ndb::page`), but real Outlook/scanpst do — implemented
/// here as a best-effort, widely-cross-referenced XOR-fold of the page's file
/// offset and BID rather than left as a placeholder. Not independently
/// verified against a real Outlook-opened PST in this environment (scanpst is
/// unavailable here) — flagged as a residual, see final report.
fn compute_page_sig(ib: u64, bid: u64) -> u16 {
    let ib32 = ib as u32;
    let bid_lo = (bid & 0xFFFF_FFFF) as u32;
    let bid_hi = (bid >> 32) as u32;
    let value = ib32 ^ bid_lo ^ bid_hi;
    ((value >> 16) ^ (value & 0xFFFF)) as u16
}

fn page_offset_map(layout: &Layout) -> HashMap<u64, u64> {
    layout.pages.iter().map(|p| (p.bid, p.offset)).collect()
}

// ── Page writers ─────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn write_bt_page<W: Write + Seek>(
    writer: &mut W,
    offset: u64,
    bid: u64,
    ptype: u8,
    c_level: u8,
    entries_region: &[u8],
    c_entries: u8,
    entry_size: usize,
) -> Result<()> {
    let mut page = vec![0u8; PAGE_SIZE as usize];
    let n = entries_region.len().min(488);
    page[..n].copy_from_slice(&entries_region[..n]);

    let c_ent_max = (488 / entry_size.max(1)).min(255) as u8;
    page[488] = c_entries;
    page[489] = c_ent_max;
    page[490] = 8; // cbEntKey
    page[491] = c_level;
    page[492..496].fill(0);

    let trailer_offset = PAGE_SIZE as usize - 16;
    page[trailer_offset] = ptype;
    page[trailer_offset + 1] = ptype;
    let sig = compute_page_sig(offset, bid);
    page[trailer_offset + 2..trailer_offset + 4].copy_from_slice(&sig.to_le_bytes());
    let crc = crc32fast::hash(&page[..trailer_offset]);
    page[trailer_offset + 4..trailer_offset + 8].copy_from_slice(&crc.to_le_bytes());
    page[trailer_offset + 8..trailer_offset + 16].copy_from_slice(&bid.to_le_bytes());

    writer.seek(SeekFrom::Start(offset))?;
    writer.write_all(&page)?;
    Ok(())
}

fn write_amap_page_v1<W: Write + Seek>(writer: &mut W, layout: &Layout) -> Result<()> {
    let amap_page = layout
        .pages
        .iter()
        .find(|p| p.ptype == PTYPE_AMAP)
        .ok_or_else(|| WriterError::Layout("missing AMap page".to_string()))?;

    let mut page = vec![0u8; PAGE_SIZE as usize];
    page[..496].fill(0xFF);

    let trailer_offset = PAGE_SIZE as usize - 16;
    page[trailer_offset] = PTYPE_AMAP;
    page[trailer_offset + 1] = PTYPE_AMAP;
    let sig = compute_page_sig(amap_page.offset, amap_page.bid);
    page[trailer_offset + 2..trailer_offset + 4].copy_from_slice(&sig.to_le_bytes());
    let crc = crc32fast::hash(&page[..trailer_offset]);
    page[trailer_offset + 4..trailer_offset + 8].copy_from_slice(&crc.to_le_bytes());
    page[trailer_offset + 8..trailer_offset + 16].copy_from_slice(&amap_page.bid.to_le_bytes());

    writer.seek(SeekFrom::Start(amap_page.offset))?;
    writer.write_all(&page)?;
    Ok(())
}

fn encode_nbt_leaf(n: &NodeEntry) -> [u8; 32] {
    let mut e = [0u8; 32];
    e[0..8].copy_from_slice(&n.nid.to_le_bytes());
    e[8..16].copy_from_slice(&n.bid_data.to_le_bytes());
    e[16..24].copy_from_slice(&n.bid_sub.to_le_bytes());
    e[24..28].copy_from_slice(&(n.nid_parent as u32).to_le_bytes());
    e
}

fn encode_bbt_leaf(b: &BlockEntry) -> [u8; 24] {
    let mut e = [0u8; 24];
    e[0..8].copy_from_slice(&b.bid.to_le_bytes());
    e[8..16].copy_from_slice(&b.offset.to_le_bytes());
    e[16..18].copy_from_slice(&(b.data.len() as u16).to_le_bytes());
    e[18..20].copy_from_slice(&1u16.to_le_bytes()); // cRef
    e
}

/// Write every level of a planned BTree (leaf pages then each intermediate
/// level up to the single root), using real sorted ascending keys (the true
/// NID/BID of the minimum entry in each child subtree) — not a placeholder.
fn write_tree<W: Write + Seek>(
    writer: &mut W,
    plan: &TreePlan,
    page_offsets: &HashMap<u64, u64>,
    leaf_min_keys: &[u64],
    leaf_pages: &[(Vec<u8>, u8)],
) -> Result<()> {
    for (i, bid) in plan.levels[0].iter().enumerate() {
        let offset = *page_offsets
            .get(bid)
            .ok_or_else(|| WriterError::Layout("missing leaf page offset".to_string()))?;
        let (region, c_entries) = &leaf_pages[i];
        write_bt_page(
            writer,
            offset,
            *bid,
            plan.ptype,
            0,
            region,
            *c_entries,
            plan.leaf_entry_size,
        )?;
    }

    let mut prev_bids = plan.levels[0].clone();
    let mut prev_min_keys: Vec<u64> = leaf_min_keys.to_vec();

    for (level_idx, level_bids) in plan.levels.iter().enumerate().skip(1) {
        let mut new_min_keys = Vec::with_capacity(level_bids.len());
        let mut child_idx = 0usize;
        for bid in level_bids {
            let end = (child_idx + INTERMEDIATE_ENTRIES_PER_PAGE).min(prev_bids.len());
            if child_idx >= end {
                break;
            }
            let mut region = Vec::with_capacity((end - child_idx) * INTERMEDIATE_ENTRY_SIZE);
            for k in child_idx..end {
                let child_bid = prev_bids[k];
                let child_offset = *page_offsets
                    .get(&child_bid)
                    .ok_or_else(|| WriterError::Layout("missing child page offset".to_string()))?;
                region.extend_from_slice(&prev_min_keys[k].to_le_bytes());
                region.extend_from_slice(&child_bid.to_le_bytes());
                region.extend_from_slice(&child_offset.to_le_bytes());
            }
            new_min_keys.push(prev_min_keys[child_idx]);
            let offset = *page_offsets.get(bid).ok_or_else(|| {
                WriterError::Layout("missing intermediate page offset".to_string())
            })?;
            write_bt_page(
                writer,
                offset,
                *bid,
                plan.ptype,
                level_idx as u8,
                &region,
                (end - child_idx) as u8,
                INTERMEDIATE_ENTRY_SIZE,
            )?;
            child_idx = end;
        }
        prev_bids = level_bids.clone();
        prev_min_keys = new_min_keys;
    }

    Ok(())
}

fn write_nbt<W: Write + Seek>(
    writer: &mut W,
    layout: &Layout,
    plan: &TreePlan,
    page_offsets: &HashMap<u64, u64>,
) -> Result<()> {
    let mut sorted: Vec<&NodeEntry> = layout.nodes.iter().collect();
    sorted.sort_by_key(|n| n.nid);

    let mut leaf_pages = Vec::new();
    let mut min_keys = Vec::new();
    for chunk in sorted.chunks(plan.per_leaf_capacity) {
        let mut region = Vec::with_capacity(chunk.len() * NBT_LEAF_ENTRY_SIZE);
        for n in chunk {
            region.extend_from_slice(&encode_nbt_leaf(n));
        }
        min_keys.push(chunk[0].nid);
        leaf_pages.push((region, chunk.len() as u8));
    }
    if leaf_pages.is_empty() {
        leaf_pages.push((Vec::new(), 0));
        min_keys.push(0);
    }

    write_tree(writer, plan, page_offsets, &min_keys, &leaf_pages)
}

fn write_bbt<W: Write + Seek>(
    writer: &mut W,
    layout: &Layout,
    plan: &TreePlan,
    page_offsets: &HashMap<u64, u64>,
) -> Result<()> {
    let mut sorted: Vec<&BlockEntry> = layout.blocks.iter().collect();
    sorted.sort_by_key(|b| b.bid);

    let mut leaf_pages = Vec::new();
    let mut min_keys = Vec::new();
    for chunk in sorted.chunks(plan.per_leaf_capacity) {
        let mut region = Vec::with_capacity(chunk.len() * BBT_LEAF_ENTRY_SIZE);
        for b in chunk {
            region.extend_from_slice(&encode_bbt_leaf(b));
        }
        min_keys.push(chunk[0].bid);
        leaf_pages.push((region, chunk.len() as u8));
    }
    if leaf_pages.is_empty() {
        leaf_pages.push((Vec::new(), 0));
        min_keys.push(0);
    }

    write_tree(writer, plan, page_offsets, &min_keys, &leaf_pages)
}

/// Production header writer — unlike `crate::write_header` (fixture path, which
/// locates the NBT/BBT/AMap root pages by linear `.find()` + `.unwrap()` and
/// only works for single-page trees), this takes the real multi-level tree
/// plans and never panics.
fn write_header_v1<W: Write>(
    writer: &mut W,
    layout: &Layout,
    nbt_plan: &TreePlan,
    bbt_plan: &TreePlan,
) -> Result<()> {
    let root_nbt_bid = *nbt_plan
        .levels
        .last()
        .and_then(|l| l.first())
        .ok_or_else(|| WriterError::Layout("empty NBT plan".to_string()))?;
    let root_bbt_bid = *bbt_plan
        .levels
        .last()
        .and_then(|l| l.first())
        .ok_or_else(|| WriterError::Layout("empty BBT plan".to_string()))?;

    let offsets = page_offset_map(layout);
    let nbt_offset = *offsets
        .get(&root_nbt_bid)
        .ok_or_else(|| WriterError::Layout("missing NBT root offset".to_string()))?;
    let bbt_offset = *offsets
        .get(&root_bbt_bid)
        .ok_or_else(|| WriterError::Layout("missing BBT root offset".to_string()))?;
    let amap_page = layout
        .pages
        .iter()
        .find(|p| p.ptype == PTYPE_AMAP)
        .ok_or_else(|| WriterError::Layout("missing AMap page".to_string()))?;

    let file_size = layout.file_size();
    let next_bid = layout.next_bid_counter;

    let mut buf = Vec::new();
    buf.write_u32::<LittleEndian>(PST_MAGIC)?;
    buf.write_u32::<LittleEndian>(0)?; // dwCRCPartial
    buf.write_u16::<LittleEndian>(CLIENT_MAGIC)?;
    buf.write_u16::<LittleEndian>(UNICODE_VERSION)?;
    buf.write_u16::<LittleEndian>(0x0036)?; // wVerClient
    buf.write_all(&[0x01, 0x01])?; // bPlatformCreate, bPlatformAccess
    buf.write_u32::<LittleEndian>(0)?;
    buf.write_u32::<LittleEndian>(0)?;
    buf.write_u64::<LittleEndian>(0)?; // bidUnused
    buf.write_u64::<LittleEndian>(next_bid)?; // bidNextP
    buf.write_u32::<LittleEndian>(1)?; // dwUnique
    buf.write_all(&[0u8; 128])?; // rgnid
    buf.write_u64::<LittleEndian>(0)?; // qwUnused

    // ROOT (72 bytes)
    buf.write_u32::<LittleEndian>(0)?;
    buf.write_u64::<LittleEndian>(file_size)?;
    buf.write_u64::<LittleEndian>(amap_page.offset)?;
    buf.write_u64::<LittleEndian>(0)?; // cbAMapFree
    buf.write_u64::<LittleEndian>(0)?; // cbPMapFree
    buf.write_u64::<LittleEndian>(root_nbt_bid)?;
    buf.write_u64::<LittleEndian>(nbt_offset)?;
    buf.write_u64::<LittleEndian>(root_bbt_bid)?;
    buf.write_u64::<LittleEndian>(bbt_offset)?;
    // MS-PST §2.2.2.5 ROOT (Unicode, 72 bytes total): fAMapValid (1) +
    // bReserved (1) + wReserved (2) = 4 bytes, matching
    // `pst_reader::header::PstHeader::read_root` exactly (see that module's
    // comment: the old 8-byte padding here was the same already-fixed-on-read
    // bug, copied verbatim from the pre-existing fixture `write_header`).
    buf.write_u8(1)?; // fAMapValid
    buf.write_all(&[0u8; 3])?; // bReserved (1) + wReserved (2)

    buf.write_u32::<LittleEndian>(0)?; // dwAlign — ends at 0x100
                                       // rgbFM (128) + rgbFP (128) = 256 bytes, ending at 0x200 — matching
                                       // `pst_reader::header::PstHeader::read` exactly (the old 508-byte skip
                                       // here was the corresponding already-fixed-on-read bug).
    buf.write_all(&[0u8; 256])?; // rgbFM + rgbFP
    buf.write_u8(0x80)?; // bSentinel (offset 0x200)
    buf.write_u8(0)?; // bCryptMethod = none (offset 0x201)
    buf.write_u16::<LittleEndian>(0)?; // rgbReserved (offset 0x202)
    buf.write_u64::<LittleEndian>(next_bid)?; // bidNextB (offset 0x204)

    let padding = (HEADER_SIZE as usize).saturating_sub(buf.len());
    buf.resize(buf.len() + padding, 0);
    writer.write_all(&buf)?;
    Ok(())
}

// ── Unit tests (verification gate: XBLOCK encode/decode symmetry) ──────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_data_chain_small_uses_single_external_block() {
        let mut layout = Layout::new();
        let bid = layout.write_data_chain(vec![1, 2, 3, 4, 5]).expect("chain");
        assert_eq!(bid & 0x02, 0, "small data should use an external block");
        assert_eq!(layout.blocks.len(), 1);
        assert_eq!(layout.blocks[0].data, vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn write_data_chain_empty_returns_null_bid() {
        let mut layout = Layout::new();
        let bid = layout.write_data_chain(Vec::new()).expect("chain");
        assert_eq!(bid, 0);
        assert_eq!(layout.blocks.len(), 0);
    }

    #[test]
    fn write_data_chain_multiblock_builds_xblock() {
        let mut layout = Layout::new();
        let data = vec![7u8; MAX_BLOCK_DATA * 3 + 10];
        let bid = layout.write_data_chain(data.clone()).expect("chain");
        assert_eq!(bid & 0x02, 0x02, "multi-block data returns an internal bid");

        // 3 full external chunks + 1 partial + 1 XBLOCK = 5 blocks.
        assert_eq!(layout.blocks.len(), 5);

        let xblock = layout
            .blocks
            .iter()
            .find(|b| b.bid == bid)
            .expect("xblock present");
        assert_eq!(xblock.data[0], 0x01, "btype");
        assert_eq!(xblock.data[1], 0x01, "cLevel = XBLOCK");
        let c_entries = u16::from_le_bytes([xblock.data[2], xblock.data[3]]);
        assert_eq!(c_entries, 4);
        let lcb_total = u32::from_le_bytes([
            xblock.data[4],
            xblock.data[5],
            xblock.data[6],
            xblock.data[7],
        ]);
        assert_eq!(lcb_total as usize, data.len());
    }

    /// PidTagMessageSize (MAPI 0x0E08) is a PtypInteger32 / PT_LONG property
    /// (MS-OXPROPS) — representable range `0..=i32::MAX`. `write_data_chain`
    /// must refuse anything larger than that with a hard `BodyTooLarge`
    /// error, not silently clamp/accept it, even though XBLOCK/XXBLOCK's own
    /// `lcbTotal` (a `u32`) could structurally describe a larger value. This
    /// is the boundary check itself, so it must fail before any XBLOCK/XXBLOCK
    /// chunking work — only the length matters, so a zero-filled `Vec` (cheap
    /// to allocate; no per-byte work needed) is enough to prove it without
    /// actually building/writing a multi-gigabyte chain.
    #[test]
    fn write_data_chain_rejects_data_larger_than_i32_max() {
        let mut layout = Layout::new();
        let data = vec![0u8; i32::MAX as usize + 1];
        let err = layout
            .write_data_chain(data)
            .expect_err("data larger than i32::MAX must be refused, not silently accepted");
        assert!(
            matches!(err, WriterError::BodyTooLarge(_)),
            "expected BodyTooLarge, got {err:?}"
        );
        assert_eq!(
            layout.blocks.len(),
            0,
            "no blocks should be written when the size ceiling is exceeded"
        );
    }
}
