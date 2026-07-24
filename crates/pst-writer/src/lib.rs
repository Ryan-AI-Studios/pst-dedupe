//! Minimal PST writer for creating Unicode PST fixtures from EML files.
//!
//! This is not a general-purpose PST writer. It creates small, unencrypted PSTs
//! with a single folder and basic message properties for testing purposes.

use std::collections::HashSet;
use std::fs::File;
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

#[cfg(test)]
mod heap_test;

use byteorder::{LittleEndian, WriteBytesExt};

pub mod eml;
pub mod production;

pub use production::{
    build_bth_checked, build_pc_v2, build_tc_inline_checked, from_canonical_message,
    temp_sibling_path, write_unicode_pst, write_unicode_pst_streaming,
    write_unicode_pst_with_streams, AttachRead, AttachStreamSource, AttachmentFidelityEvent,
    AttachmentFidelityKind, FolderLayoutPolicy, PcValue, WriteAttachment, WriteMessage,
    WriteProgress, WriteProgressSink, WritePstOpts, WritePstReport, WriteStage,
};

// EagerWriteCtx is defined above on Layout; re-exported for tests/integrations.

// ── Constants ──────────────────────────────────────────────────────────────────

pub(crate) const PST_MAGIC: u32 = 0x4E444221; // "!BDN" LE
pub(crate) const CLIENT_MAGIC: u16 = 0x4D53; // "SM"
pub(crate) const UNICODE_VERSION: u16 = 23;

/// Header size: 564 bytes padded to 4096.
pub(crate) const HEADER_SIZE: u64 = 4096;
/// Page size: always 512 bytes.
pub(crate) const PAGE_SIZE: u64 = 512;
/// Max payload in a data block (8192 - 16 trailer).
pub(crate) const MAX_BLOCK_DATA: usize = 8176;
/// Block alignment: 64 bytes.
pub(crate) const BLOCK_ALIGN: u64 = 64;

/// MS-PST Allocation Map (AMap): first AMap page at absolute file offset
/// `0x4400` (17408). See MS-PST “Allocation Map page” / Unicode NDB layout.
///
/// Public for production streaming scale (track 0070) and tests.
pub const AMAP_FIRST_OFFSET: u64 = 0x4400;
/// MS-PST: subsequent AMap pages every **253 952** (`0x3E000`) bytes.
pub const AMAP_INTERVAL: u64 = 253_952; // 0x3E000

/// Page type for Allocation Map pages.
pub(crate) const PTYPE_AMAP: u8 = 0x84;

/// Return true when `offset` is a mandated AMap page slot.
#[inline]
pub fn is_amap_page_offset(offset: u64) -> bool {
    if offset < AMAP_FIRST_OFFSET {
        return false;
    }
    (offset - AMAP_FIRST_OFFSET).is_multiple_of(AMAP_INTERVAL)
}

/// Next AMap page absolute offset at or after `offset`.
#[inline]
pub fn next_amap_at_or_after(offset: u64) -> u64 {
    if offset <= AMAP_FIRST_OFFSET {
        return AMAP_FIRST_OFFSET;
    }
    let rel = offset - AMAP_FIRST_OFFSET;
    let rem = rel % AMAP_INTERVAL;
    if rem == 0 {
        offset
    } else {
        offset + (AMAP_INTERVAL - rem)
    }
}

// ── NID Constants ────────────────────────────────────────────────────────────

pub(crate) const NID_MESSAGE_STORE: u64 = 0x21;
pub(crate) const NID_NAME_TO_ID_MAP: u64 = 0x61;
pub(crate) const NID_ROOT_FOLDER: u64 = 0x122;

// Fixed MS-PST "template object" NIDs (track 0068 round 9 — verified against
// learn.microsoft.com MS-PST pages; see `production::write_unicode_pst`
// doc comments for the exact sources). These are absolute, fixed NIDs — NOT
// derived via `Layout::alloc_nid` — one top-level node per template, each
// containing a TCINFO column schema with zero data rows.
pub(crate) const NID_HIERARCHY_TABLE_TEMPLATE: u64 = 0x60D;
pub(crate) const NID_CONTENTS_TABLE_TEMPLATE: u64 = 0x60E;
pub(crate) const NID_ASSOC_CONTENTS_TABLE_TEMPLATE: u64 = 0x60F;
pub(crate) const NID_SEARCH_CONTENTS_TABLE_TEMPLATE: u64 = 0x610;
/// Attachment Table Template (MS-PST fixed NID). Zero-row TC with the
/// attachment-table column schema; per-message attachment tables also use
/// this NID as their subnode key under the message's subnode BTree.
pub(crate) const NID_ATTACHMENT_TABLE_TEMPLATE: u64 = 0x671;

// NID types
pub(crate) const NID_TYPE_NORMAL_FOLDER: u8 = 0x02;
pub(crate) const NID_TYPE_SEARCH_FOLDER: u8 = 0x03;
pub(crate) const NID_TYPE_NORMAL_MESSAGE: u8 = 0x04;

// ── Property Tags ──────────────────────────────────────────────────────────

pub(crate) const PID_TAG_DISPLAY_NAME: u16 = 0x3001;
pub(crate) const PID_TAG_SUBJECT: u16 = 0x0037;
pub(crate) const PID_TAG_CLIENT_SUBMIT_TIME: u16 = 0x0039;
pub(crate) const PID_TAG_SENDER_EMAIL_ADDRESS: u16 = 0x0C1F;
pub(crate) const PID_TAG_INTERNET_MESSAGE_ID: u16 = 0x1035;
pub(crate) const PID_TAG_BODY: u16 = 0x1000;
pub(crate) const PID_TAG_MESSAGE_SIZE: u16 = 0x0E08;
pub(crate) const PID_TAG_HAS_ATTACHMENTS: u16 = 0x0E1B;
pub(crate) const PID_TAG_CONTENT_COUNT: u16 = 0x3602;
pub(crate) const PID_TAG_LTP_ROW_ID: u16 = 0x67F2;

// ── Property Types ─────────────────────────────────────────────────────────

pub(crate) const PTYP_INTEGER_32: u16 = 0x0003;
pub(crate) const PTYP_BOOLEAN: u16 = 0x000B;
pub(crate) const PTYP_INTEGER_64: u16 = 0x0014;
pub(crate) const PTYP_STRING: u16 = 0x001F;
pub(crate) const PTYP_TIME: u16 = 0x0040;

// ── Error Type ───────────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum WriterError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("EML parse error: {0}")]
    EmlParse(String),
    #[error("Layout error: {0}")]
    Layout(String),
    /// A single node/subnode value could not be represented within documented
    /// XBLOCK/XXBLOCK capacity limits (see `production` module docs).
    #[error("body/value too large to represent: {0}")]
    BodyTooLarge(String),
    /// XBLOCK/XXBLOCK chain allocation exceeded documented capacity limits.
    #[error("allocation failed: {0}")]
    AllocationFailed(String),
    /// Output safety refusal (e.g. destination exists without opt-in overwrite).
    #[error("refused: {0}")]
    Refused(String),
    /// Hard, non-overridable refusal: a path this writer is about to write
    /// bytes to — either the final destination, or its computed temp-staging
    /// sibling (see `production::temp_sibling_path`) — matches a
    /// caller-declared protected *source* input PST (the mandatory
    /// `protected_source_paths` parameter of `write_unicode_pst`). Checked
    /// for both paths, independently, before either is ever passed to
    /// `File::create`.
    /// Unlike [`WriterError::Refused`], `WritePstOpts::overwrite = true` never
    /// bypasses this — this project is read-only against PST inputs (see
    /// spec §3.7 rule 1 / Core Mandate #3) and there is no legitimate reason to
    /// ever write onto a known input path.
    #[error("refused: {0} is a protected source input PST; refusing to write onto it (this check cannot be bypassed by `overwrite`)")]
    RefusedSourceOverwrite(PathBuf),
}

pub type Result<T> = std::result::Result<T, WriterError>;

// ── Layout Engine ──────────────────────────────────────────────────────────

/// Eager same-directory temp writer for multi-GB streaming (track 0070).
///
/// Leaf data blocks are placed with AMap-aware allocation and written to the
/// temp file immediately so [`Layout`] retains only thin BBT metadata
/// (`bid` / `offset` / `len`, `on_disk = true`, empty `data`).
#[derive(Debug)]
pub struct EagerWriteCtx {
    pub(crate) file: File,
    /// Next free offset for placement (always ≥ [`HEADER_SIZE`]).
    pub(crate) cursor: u64,
    /// AMap pages registered while placing eager blocks (stubs written).
    pub(crate) amap_pages: Vec<PageEntry>,
    /// Absolute offsets of AMap stubs already written to `file`.
    pub(crate) amap_stubs_written: HashSet<u64>,
}

impl EagerWriteCtx {
    /// Create the same-dir temp file with a zeroed header placeholder.
    pub fn create(path: &Path) -> Result<Self> {
        let mut file = File::create(path).map_err(|e| {
            WriterError::Io(std::io::Error::new(
                e.kind(),
                format!(
                    "failed to create same-directory temp {} (required for atomic rename; \
                     no cross-volume multi-GB copy fallback): {e}",
                    path.display()
                ),
            ))
        })?;
        let header = vec![0u8; HEADER_SIZE as usize];
        file.write_all(&header)?;
        file.flush()?;
        Ok(Self {
            file,
            cursor: HEADER_SIZE,
            amap_pages: Vec::new(),
            amap_stubs_written: HashSet::new(),
        })
    }

    /// Best-effort cumulative physical size (write cursor vs file metadata).
    pub fn physical_size(&self) -> u64 {
        let meta = self.file.metadata().map(|m| m.len()).unwrap_or(0);
        meta.max(self.cursor)
    }
}

/// Tracks file offsets, BIDs, and NIDs for a pre-calculated PST layout.
#[derive(Debug)]
pub struct Layout {
    pub nodes: Vec<NodeEntry>,
    pub blocks: Vec<BlockEntry>,
    pub pages: Vec<PageEntry>,
    pub next_bid_counter: u64,
    pub next_nid_index: u32,
    pub used_bids: HashSet<u64>,
    /// When set, leaf data blocks from the production chain writers are spilled
    /// immediately to the temp file (`on_disk = true`).
    pub eager: Option<EagerWriteCtx>,
}

#[derive(Debug, Clone, Copy)]
pub struct NodeEntry {
    pub nid: u64,
    pub bid_data: u64,
    pub bid_sub: u64,
    pub nid_parent: u64,
}

#[derive(Debug, Clone)]
pub struct BlockEntry {
    pub bid: u64,
    /// Payload bytes. May be empty when the block was already written to the
    /// temp file (`on_disk = true`) — multi-GB streaming path retains only thin
    /// BBT metadata (`len` + `offset`).
    pub data: Vec<u8>,
    pub offset: u64,
    /// Payload length in bytes (excluding the 16-byte block trailer). Always
    /// authoritative for BBT `cb` and layout sizing — use this, not
    /// `data.len()`, when `data` may be empty.
    pub len: u32,
    /// When true, payload+trailer already live at `offset` on the staging file;
    /// finalize must not rewrite the block body.
    pub on_disk: bool,
}

impl BlockEntry {
    /// Create an in-memory block entry (offset filled by `calculate_offsets`).
    pub fn in_memory(bid: u64, data: Vec<u8>) -> Self {
        let len = data.len() as u32;
        Self {
            bid,
            data,
            offset: 0,
            len,
            on_disk: false,
        }
    }

    /// Thin BBT entry for a leaf already written to the staging temp file.
    pub fn on_disk(bid: u64, offset: u64, len: u32) -> Self {
        Self {
            bid,
            data: Vec::new(),
            offset,
            len,
            on_disk: true,
        }
    }

    /// Authoritative payload length.
    #[inline]
    pub fn payload_len(&self) -> usize {
        self.len as usize
    }
}

#[derive(Debug, Clone, Copy)]
pub struct PageEntry {
    pub bid: u64,
    pub ptype: u8,
    pub offset: u64,
}

impl Default for Layout {
    fn default() -> Self {
        Self::new()
    }
}

impl Layout {
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            blocks: Vec::new(),
            pages: Vec::new(),
            next_bid_counter: 0x10,
            next_nid_index: 11, // reserve 1-10 for store, named map, root folder, etc.
            used_bids: HashSet::new(),
            eager: None,
        }
    }

    /// Attach an eager temp writer (multi-GB streaming path).
    pub fn attach_eager(&mut self, ctx: EagerWriteCtx) {
        self.eager = Some(ctx);
    }

    /// Detach the eager writer (e.g. for finalize seeks / rename).
    pub fn take_eager(&mut self) -> Option<EagerWriteCtx> {
        self.eager.take()
    }

    /// Current physical size when eager is active, else 0.
    pub fn current_physical_size(&self) -> u64 {
        self.eager
            .as_ref()
            .map(EagerWriteCtx::physical_size)
            .unwrap_or(0)
    }

    /// Place a leaf block with AMap awareness and write it to the eager temp.
    /// Returns the absolute file offset. Caller should push
    /// [`BlockEntry::on_disk`] and drop the payload.
    pub fn place_and_write_block(
        &mut self,
        eager: &mut EagerWriteCtx,
        bid: u64,
        payload: &[u8],
    ) -> Result<u64> {
        let block_size = align_up(payload.len() as u64, BLOCK_ALIGN) + 16;
        let offset = amap_place_region(
            &mut eager.cursor,
            block_size,
            BLOCK_ALIGN,
            &mut eager.amap_pages,
            &mut self.used_bids,
            &mut self.next_bid_counter,
        );
        // Write AMap page stubs for any newly registered slots so physical size
        // and file holes stay consistent with MS-PST layout.
        let stubs_to_write: Vec<(u64, u64)> = eager
            .amap_pages
            .iter()
            .filter(|p| !eager.amap_stubs_written.contains(&p.offset))
            .map(|p| (p.offset, p.bid))
            .collect();
        for (amap_off, amap_bid) in stubs_to_write {
            write_amap_stub_page(&mut eager.file, amap_off, amap_bid)?;
            eager.amap_stubs_written.insert(amap_off);
        }
        eager.file.seek(SeekFrom::Start(offset))?;
        write_data_block(&mut eager.file, bid, payload)?;
        Ok(offset)
    }

    /// Store a leaf data block: spill to eager temp when present, else hold in RAM.
    pub(crate) fn push_leaf_block(&mut self, bid: u64, data: Vec<u8>) -> Result<()> {
        let len = data.len() as u32;
        if self.eager.is_none() {
            self.blocks.push(BlockEntry::in_memory(bid, data));
            return Ok(());
        }
        // Take eager out so we can mutably borrow used_bids / next_bid_counter.
        let mut eager = self.eager.take().ok_or_else(|| {
            WriterError::Layout("eager writer missing after is_some check".into())
        })?;
        let offset = self.place_and_write_block(&mut eager, bid, &data)?;
        self.blocks.push(BlockEntry::on_disk(bid, offset, len));
        self.eager = Some(eager);
        // `data` dropped here — not retained in Layout.
        Ok(())
    }

    pub(crate) fn alloc_bid(&mut self, internal: bool) -> u64 {
        loop {
            let bid = self.next_bid_counter;
            self.next_bid_counter += 1;
            let result = if internal { bid | 0x02 } else { bid & !0x02 };
            if self.used_bids.insert(result) {
                return result;
            }
        }
    }

    pub fn alloc_nid(&mut self, nid_type: u8) -> u64 {
        let nid = ((self.next_nid_index as u64) << 5) | (nid_type as u64);
        self.next_nid_index += 1;
        nid
    }

    /// Add a node with its data block.
    pub fn add_node(&mut self, nid: u64, data: Vec<u8>, nid_parent: u64) -> u64 {
        assert!(
            data.len() <= MAX_BLOCK_DATA,
            "block data {} bytes exceeds MAX_BLOCK_DATA ({}). nid=0x{:X}",
            data.len(),
            MAX_BLOCK_DATA,
            nid
        );
        let bid_data = self.alloc_bid(false);
        self.nodes.push(NodeEntry {
            nid,
            bid_data,
            bid_sub: 0,
            nid_parent,
        });
        self.blocks.push(BlockEntry::in_memory(bid_data, data));
        bid_data
    }

    /// Reserve a page (non-AMap). AMap pages are placed only at fixed MS-PST
    /// offsets by [`Self::calculate_offsets`] — do not reserve floating AMaps.
    pub fn reserve_page(&mut self, ptype: u8) -> u64 {
        let bid = self.alloc_bid(true);
        self.pages.push(PageEntry {
            bid,
            ptype,
            offset: 0,
        });
        bid
    }

    /// Calculate final file offsets for all blocks and pages.
    ///
    /// **AMap-aware (track 0070 / MS-PST):** data blocks and B-Tree pages never
    /// land on fixed AMap slots (`AMAP_FIRST_OFFSET`, then every
    /// `AMAP_INTERVAL`). When sequential placement would cross or land on an
    /// AMap page, the allocator reserves the map page at that absolute offset
    /// and resumes after it. Valid AMap page content is written later; free-bit
    /// accounting may be approximate (all-free `0xFF` is acceptable for v1).
    ///
    /// **Eager on_disk ordering:** when leaf blocks were already placed/written
    /// to the temp file, the cursor starts at the end of that region. NBT/BBT
    /// pages and remaining in-memory blocks are placed *after* those blocks so
    /// they never collide with pre-written data. AMap pages at mandated slots
    /// (including those skipped during eager) are ensured through `file_end`.
    pub fn calculate_offsets(&mut self) {
        // Non-AMap pages (NBT/BBT). Eager AMap stubs are re-registered below
        // (or rewritten at finalize with consistent BIDs).
        let mut other_pages: Vec<PageEntry> = self
            .pages
            .drain(..)
            .filter(|p| p.ptype != PTYPE_AMAP)
            .collect();

        // Reuse AMap PageEntries already allocated during eager placement so
        // stub BIDs stay consistent if we re-write the same pages.
        let mut amap_pages: Vec<PageEntry> = if let Some(eager) = self.eager.as_mut() {
            std::mem::take(&mut eager.amap_pages)
        } else {
            Vec::new()
        };

        // 1) Start after all eagerly written on_disk blocks (and header).
        let mut cursor = HEADER_SIZE;
        if let Some(eager) = self.eager.as_ref() {
            cursor = cursor.max(eager.cursor);
        }
        for block in &self.blocks {
            if block.on_disk && block.offset != 0 {
                let block_size = align_up(block.payload_len() as u64, BLOCK_ALIGN) + 16;
                cursor = cursor.max(block.offset + block_size);
            }
        }

        // 2) Place remaining non-AMap pages after the on_disk region.
        for page in &mut other_pages {
            if page.offset != 0 {
                cursor = cursor.max(page.offset + PAGE_SIZE);
                continue;
            }
            page.offset = amap_place_region(
                &mut cursor,
                PAGE_SIZE,
                1,
                &mut amap_pages,
                &mut self.used_bids,
                &mut self.next_bid_counter,
            );
        }

        // 3) Place remaining in-memory blocks after pages.
        for block in &mut self.blocks {
            if block.on_disk && block.offset != 0 {
                continue;
            }
            let block_size = align_up(block.payload_len() as u64, BLOCK_ALIGN) + 16;
            block.offset = amap_place_region(
                &mut cursor,
                block_size,
                BLOCK_ALIGN,
                &mut amap_pages,
                &mut self.used_bids,
                &mut self.next_bid_counter,
            );
        }

        // 4) Ensure every mandated AMap slot below the provisional file end exists.
        let mut file_end = cursor;
        if file_end <= AMAP_FIRST_OFFSET {
            file_end = AMAP_FIRST_OFFSET + PAGE_SIZE;
        }
        loop {
            let mut changed = false;
            let mut amap = AMAP_FIRST_OFFSET;
            while amap < file_end {
                let before = amap_pages.len();
                amap_ensure_page(
                    amap,
                    &mut amap_pages,
                    &mut self.used_bids,
                    &mut self.next_bid_counter,
                );
                if amap_pages.len() > before {
                    changed = true;
                }
                file_end = file_end.max(amap + PAGE_SIZE);
                amap += AMAP_INTERVAL;
            }
            if !changed {
                break;
            }
        }

        amap_pages.sort_by_key(|p| p.offset);
        self.pages = amap_pages;
        self.pages.extend(other_pages);
    }

    pub fn file_size(&self) -> u64 {
        let mut max = HEADER_SIZE;
        for page in &self.pages {
            max = max.max(page.offset + PAGE_SIZE);
        }
        for block in &self.blocks {
            let block_size = align_up(block.payload_len() as u64, BLOCK_ALIGN) + 16;
            max = max.max(block.offset + block_size);
        }
        max
    }

    /// Highest AMap page offset, if any.
    pub fn ib_amap_last(&self) -> Option<u64> {
        self.pages
            .iter()
            .filter(|p| p.ptype == PTYPE_AMAP)
            .map(|p| p.offset)
            .max()
    }
}

pub(crate) fn align_up(value: u64, alignment: u64) -> u64 {
    value.div_ceil(alignment) * alignment
}

/// Register an AMap page at a fixed absolute offset if not already present.
pub(crate) fn amap_ensure_page(
    amap_off: u64,
    amap_pages: &mut Vec<PageEntry>,
    used_bids: &mut HashSet<u64>,
    next_bid: &mut u64,
) {
    if amap_pages.iter().any(|p| p.offset == amap_off) {
        return;
    }
    let bid = loop {
        let b = *next_bid;
        *next_bid += 1;
        let result = b | 0x02;
        if used_bids.insert(result) {
            break result;
        }
    };
    amap_pages.push(PageEntry {
        bid,
        ptype: PTYPE_AMAP,
        offset: amap_off,
    });
}

/// Place a region of `size` bytes with `align` alignment, never overlapping
/// mandated AMap page slots. Registers AMap pages when the cursor must skip
/// past them.
pub(crate) fn amap_place_region(
    cursor: &mut u64,
    size: u64,
    align: u64,
    amap_pages: &mut Vec<PageEntry>,
    used_bids: &mut HashSet<u64>,
    next_bid: &mut u64,
) -> u64 {
    *cursor = align_up(*cursor, align);
    loop {
        if is_amap_page_offset(*cursor) {
            amap_ensure_page(*cursor, amap_pages, used_bids, next_bid);
            *cursor = align_up(*cursor + PAGE_SIZE, align);
            continue;
        }
        let amap = next_amap_at_or_after(*cursor);
        let region_end = *cursor + size;
        let amap_end = amap + PAGE_SIZE;
        if *cursor < amap_end && region_end > amap {
            if *cursor < amap && region_end <= amap {
                break;
            }
            amap_ensure_page(amap, amap_pages, used_bids, next_bid);
            *cursor = align_up(amap_end, align);
            continue;
        }
        break;
    }
    let offset = *cursor;
    *cursor += size;
    offset
}

/// Write a provisional AMap page (all-free `0xFF` bits) at a fixed offset.
/// Finalized AMap content may be rewritten at finalize with the same layout.
fn write_amap_stub_page(file: &mut File, offset: u64, bid: u64) -> Result<()> {
    let mut page = vec![0u8; PAGE_SIZE as usize];
    page[..496].fill(0xFF);
    let trailer_offset = PAGE_SIZE as usize - 16;
    page[trailer_offset] = PTYPE_AMAP;
    page[trailer_offset + 1] = PTYPE_AMAP;
    // Best-effort wSig (same fold as production finalize).
    let ib32 = offset as u32;
    let bid_lo = (bid & 0xFFFF_FFFF) as u32;
    let bid_hi = (bid >> 32) as u32;
    let value = ib32 ^ bid_lo ^ bid_hi;
    let sig = ((value >> 16) ^ (value & 0xFFFF)) as u16;
    page[trailer_offset + 2..trailer_offset + 4].copy_from_slice(&sig.to_le_bytes());
    let crc = crc32fast::hash(&page[..trailer_offset]);
    page[trailer_offset + 4..trailer_offset + 8].copy_from_slice(&crc.to_le_bytes());
    page[trailer_offset + 8..trailer_offset + 16].copy_from_slice(&bid.to_le_bytes());
    file.seek(SeekFrom::Start(offset))?;
    file.write_all(&page)?;
    Ok(())
}

// ── Builders ───────────────────────────────────────────────────────────────

/// Build a Unicode PST header.
pub fn write_header<W: Write>(writer: &mut W, layout: &Layout) -> Result<()> {
    let file_size = layout.file_size();
    let nbt_root = layout.pages.iter().find(|p| p.ptype == 0x81).unwrap(); // NBT intermediate
    let bbt_root = layout.pages.iter().find(|p| p.ptype == 0x80).unwrap(); // BBT intermediate
    let amap_last = layout
        .ib_amap_last()
        .or_else(|| {
            layout
                .pages
                .iter()
                .find(|p| p.ptype == PTYPE_AMAP)
                .map(|p| p.offset)
        })
        .unwrap_or(AMAP_FIRST_OFFSET);

    let mut buf = Vec::new();

    // dwMagic (4)
    buf.write_u32::<LittleEndian>(PST_MAGIC)?;
    // dwCRCPartial (4) — skip, write 0
    buf.write_u32::<LittleEndian>(0)?;
    // wMagicClient (2)
    buf.write_u16::<LittleEndian>(CLIENT_MAGIC)?;
    // wVer (2)
    buf.write_u16::<LittleEndian>(UNICODE_VERSION)?;
    // wVerClient (2)
    buf.write_u16::<LittleEndian>(0x0036)?;
    // bPlatformCreate (1) + bPlatformAccess (1)
    buf.write_all(&[0x01, 0x01])?;
    // dwReserved1 (4) + dwReserved2 (4)
    buf.write_u32::<LittleEndian>(0)?;
    buf.write_u32::<LittleEndian>(0)?;
    // bidUnused (8)
    buf.write_u64::<LittleEndian>(0)?;
    // bidNextP (8)
    let next_p = layout.next_bid_counter;
    buf.write_u64::<LittleEndian>(next_p)?;
    // dwUnique (4)
    buf.write_u32::<LittleEndian>(1)?;
    // rgnid[32] (128) — skip, zeros
    buf.write_all(&[0u8; 128])?;
    // qwUnused (8)
    buf.write_u64::<LittleEndian>(0)?;

    // ROOT structure (72 bytes)
    // dwReserved (4)
    buf.write_u32::<LittleEndian>(0)?;
    // ibFileEof (8)
    buf.write_u64::<LittleEndian>(file_size)?;
    // ibAMapLast (8)
    buf.write_u64::<LittleEndian>(amap_last)?;
    // cbAMapFree (8)
    buf.write_u64::<LittleEndian>(0)?; // no free space
                                       // cbPMapFree (8)
    buf.write_u64::<LittleEndian>(0)?;
    // brefNBT (16)
    buf.write_u64::<LittleEndian>(nbt_root.bid)?;
    buf.write_u64::<LittleEndian>(nbt_root.offset)?;
    // brefBBT (16)
    buf.write_u64::<LittleEndian>(bbt_root.bid)?;
    buf.write_u64::<LittleEndian>(bbt_root.offset)?;
    // fAMapValid (1)
    buf.write_u8(1)?;
    // padding (7)
    buf.write_all(&[0u8; 7])?;

    // dwAlign (4)
    buf.write_u32::<LittleEndian>(0)?;
    // rgbFM (380) + rgbFP (128) = 508 bytes
    buf.write_all(&[0u8; 508])?;
    // bSentinel (1)
    buf.write_u8(0x80)?;
    // bCryptMethod (1) — 0 = none
    buf.write_u8(0)?;
    // rgbReserved (2)
    buf.write_u16::<LittleEndian>(0)?;
    // bidNextB (8)
    let next_b = layout.next_bid_counter;
    buf.write_u64::<LittleEndian>(next_b)?;

    // Pad to HEADER_SIZE
    let padding = (HEADER_SIZE as usize).saturating_sub(buf.len());
    buf.resize(buf.len() + padding, 0);

    writer.write_all(&buf)?;
    Ok(())
}

// ── Page Writer ──────────────────────────────────────────────────────────────

/// Write a B-tree leaf page.
pub fn write_btree_leaf_page<W: Write>(
    writer: &mut W,
    page_bid: u64,
    ptype: u8,
    entries: &[u8],
) -> Result<()> {
    let mut page_data = vec![0u8; PAGE_SIZE as usize];

    // Copy entries into first 488 bytes
    let entry_len = entries.len().min(488);
    page_data[..entry_len].copy_from_slice(&entries[..entry_len]);

    // BTPAGE header at offset 488
    let c_entries = entry_len as u8;
    let c_ent_max = (488u16 / (c_entries as u16).max(1)) as u8;
    page_data[488] = c_entries;
    page_data[489] = c_ent_max;
    page_data[490] = 8; // cbEntKey
    page_data[491] = 0; // cLevel = leaf
                        // dwPadding (4)
    page_data[492..496].fill(0);

    // Page trailer at offset 496 (last 16 bytes)
    let trailer_offset = PAGE_SIZE as usize - 16;
    page_data[trailer_offset] = ptype;
    page_data[trailer_offset + 1] = ptype; // ptypeRepeat
                                           // wSig
    let wsig = compute_page_signature(page_bid, ptype);
    page_data[trailer_offset + 2..trailer_offset + 4].copy_from_slice(&wsig.to_le_bytes());
    // dwCRC
    let crc = crc32fast::hash(&page_data[..trailer_offset]);
    page_data[trailer_offset + 4..trailer_offset + 8].copy_from_slice(&crc.to_le_bytes());
    // bid
    page_data[trailer_offset + 8..trailer_offset + 16].copy_from_slice(&page_bid.to_le_bytes());

    writer.write_all(&page_data)?;
    Ok(())
}

/// Write a B-tree intermediate page.
pub fn write_btree_intermediate_page<W: Write>(
    _writer: &mut W,
    _page_bid: u64,
    _ptype: u8,
    child_brefs: &[(u64, u64)], // (key, child_page_offset)
) -> Result<()> {
    let mut page_data = vec![0u8; PAGE_SIZE as usize];

    // Entries: key(8) + BREF(16) = 24 bytes each
    let mut offset = 0;
    for (key, _child_offset) in child_brefs {
        if offset + 24 > 488 {
            break;
        }
        page_data[offset..offset + 8].copy_from_slice(&key.to_le_bytes());
        // BREF: bid + ib. We need the child's bid.
        // For intermediate pages, we store the child page's BID.
        // But we only have offset here. We'll need to pass bid too.
        offset += 24;
    }

    // For simplicity, intermediate pages need child BIDs.
    // Let me redesign this function signature.
    unimplemented!("intermediate page needs child BIDs")
}

fn compute_page_signature(_bid: u64, _ptype: u8) -> u16 {
    // wSig computation from MS-PST is complex. For our minimal writer,
    // we can use a placeholder. The reader may or may not validate it.
    0
}

// ── Block Writer ───────────────────────────────────────────────────────────

/// Write an external data block with trailer.
pub fn write_data_block<W: Write>(writer: &mut W, bid: u64, data: &[u8]) -> Result<()> {
    let aligned_len = align_up(data.len() as u64, BLOCK_ALIGN) as usize;
    let mut block = vec![0u8; aligned_len + 16];

    // Data
    block[..data.len()].copy_from_slice(data);

    // Trailer at end
    let trailer_offset = aligned_len;
    // dwCRC
    let crc = crc32fast::hash(data);
    block[trailer_offset..trailer_offset + 4].copy_from_slice(&crc.to_le_bytes());
    // bid
    block[trailer_offset + 4..trailer_offset + 12].copy_from_slice(&bid.to_le_bytes());
    // padding (4)
    block[trailer_offset + 12..trailer_offset + 16].fill(0);

    writer.write_all(&block)?;
    Ok(())
}

// ── Heap Builder ─────────────────────────────────────────────────────────────

/// A simple heap-on-node builder for a single-block HN.
#[derive(Debug)]
pub struct HeapBuilder {
    pub data: Vec<u8>,
    allocations: Vec<(usize, usize)>, // (start, end) offsets of each allocation
}

impl HeapBuilder {
    pub fn new(client_sig: u8) -> Self {
        let mut data = Vec::new();
        // HNHDR (12 bytes)
        data.extend_from_slice(&0u16.to_le_bytes()); // ibHnpm placeholder
        data.push(0xEC); // bSig
        data.push(client_sig); // bClientSig
        data.extend_from_slice(&0u32.to_le_bytes()); // hidUserRoot placeholder
        data.extend_from_slice(&0u32.to_le_bytes()); // rgbFillLevel

        Self {
            data,
            allocations: Vec::new(),
        }
    }

    /// Allocate a chunk and return its full HID value.
    pub fn alloc(&mut self, bytes: &[u8]) -> u32 {
        let start = self.data.len();
        self.data.extend_from_slice(bytes);
        let end = self.data.len();
        let index = (self.allocations.len() as u32) + 1; // 1-based
        self.allocations.push((start, end));
        index << 5 // Full HID: hid_type=0, hid_block_index=0, hid_index=index
    }

    /// Allocate a chunk, but refuse (typed error) rather than silently produce an
    /// oversized/corrupt single-page heap when the projected page (header +
    /// content so far + this allocation + the not-yet-written HN page map) would
    /// exceed one physical data block (`MAX_BLOCK_DATA`).
    ///
    /// Used by the production write path (`production` module) — callers that hit
    /// this error should divert the value to a subnode instead of inlining it.
    pub fn try_alloc(&mut self, bytes: &[u8]) -> Result<u32> {
        // HNPAGEMAP = cAlloc(2) + cFree(2) + rgibAlloc[(cAlloc+1) * 2].
        // Bound conservatively for the allocation being added.
        let projected_alloc_count = self.allocations.len() + 1;
        let projected_pagemap = 4 + (projected_alloc_count + 1) * 2;
        let projected = self.data.len() + bytes.len() + projected_pagemap;
        if projected > MAX_BLOCK_DATA {
            return Err(WriterError::Layout(format!(
                "heap page overflow: {projected} bytes would exceed single-block capacity {MAX_BLOCK_DATA} \
                 (value should be diverted to a subnode instead of inlined)"
            )));
        }
        Ok(self.alloc(bytes))
    }

    /// Overwrite the 4-byte little-endian value at `field_offset` within the
    /// allocation identified by `hid` (as returned by `alloc`/`try_alloc`).
    /// Used to back-patch forward references (e.g. BTH `hidRoot`, TCINFO
    /// `hnidRows`) once the referenced allocation's HID is known, without
    /// exposing the private `allocations` bookkeeping to other modules.
    ///
    /// Returns a typed [`WriterError::Layout`] (never silently no-ops) if `hid`
    /// does not identify a known allocation, or if `field_offset..field_offset+4`
    /// does not fit within that allocation's byte range. Not reachable today
    /// (every caller patches a HID it allocated moments earlier in the same
    /// heap), but production-path discipline is `Result` everywhere, not a
    /// silent no-op on an out-of-range index.
    pub fn patch_u32(&mut self, hid: u32, field_offset: usize, value: u32) -> Result<()> {
        let index = ((hid >> 5).saturating_sub(1)) as usize;
        let (start, end) = *self.allocations.get(index).ok_or_else(|| {
            WriterError::Layout(format!(
                "patch_u32: hid 0x{hid:X} does not identify a known heap allocation \
                 (index {index} out of range)"
            ))
        })?;
        let at = start + field_offset;
        if at + 4 > end {
            return Err(WriterError::Layout(format!(
                "patch_u32: field_offset {field_offset} (byte range {at}..{}) does not fit \
                 within allocation hid 0x{hid:X}'s bytes ({start}..{end})",
                at + 4
            )));
        }
        self.data[at..at + 4].copy_from_slice(&value.to_le_bytes());
        Ok(())
    }

    /// Finalize the heap: write page map and patch HNHDR.
    pub fn finalize(&mut self, hid_user_root: u32) -> Vec<u8> {
        let hnpm_offset = self.data.len();

        // HNPAGEMAP (MS-PST §2.3.1.5): cAlloc(2) + cFree(2) + rgibAlloc[(cAlloc+1) × 2].
        // NOTE: this previously omitted `cFree`, which shifted every `rgibAlloc`
        // read by one slot (the reader — `pst_reader::ltp::hn::Heap::get` —
        // always reads cAlloc(2)+cFree(2) before rgibAlloc) and silently
        // resolved every HID to the *next* allocation's bytes instead of its
        // own. Fixed as part of track 0068 (found while building the
        // production write path's round-trip tests).
        let c_alloc = self.allocations.len() as u16;
        self.data.extend_from_slice(&c_alloc.to_le_bytes());
        self.data.extend_from_slice(&0u16.to_le_bytes()); // cFree — unused by the reader
                                                          // rgibAlloc[0] = start of allocatable space = 12 (after HNHDR)
        self.data.extend_from_slice(&12u16.to_le_bytes());
        // rgibAlloc[i] = end of allocation i
        for (_, end) in &self.allocations {
            self.data.extend_from_slice(&(*end as u16).to_le_bytes());
        }

        // Patch ibHnpm at offset 0
        self.data[..2].copy_from_slice(&(hnpm_offset as u16).to_le_bytes());

        // Patch hidUserRoot at offset 4
        self.data[4..8].copy_from_slice(&hid_user_root.to_le_bytes());

        self.data.clone()
    }
}

// ── BTH Builder ──────────────────────────────────────────────────────────────

/// Build a BTree-on-Heap inside an existing HeapBuilder.
pub fn build_bth(
    heap: &mut HeapBuilder,
    cb_key: u8,
    cb_ent: u8,
    records: &mut [(u16, Vec<u8>)], // (key, data)
) -> u32 {
    // Sort records by key
    records.sort_by_key(|r| r.0);

    // BTH header (8 bytes)
    let mut bth_data = vec![0xB5, cb_key, cb_ent, 0]; // bType, cbKey, cbEnt, bIdxLevels
    bth_data.extend_from_slice(&0u32.to_le_bytes()); // hidRoot placeholder

    let hid_root = heap.alloc(&bth_data);
    let hid_root_index = ((hid_root >> 5) - 1) as usize;

    // Leaf records
    let mut leaf_data = Vec::new();
    for (key, data) in records {
        leaf_data.extend_from_slice(&key.to_le_bytes());
        leaf_data.extend_from_slice(data);
    }

    let hid_leaf = heap.alloc(&leaf_data);

    // Patch hidRoot in the BTH header allocation
    let bth_start = heap.allocations[hid_root_index].0;
    heap.data[bth_start + 4..bth_start + 8].copy_from_slice(&hid_leaf.to_le_bytes());

    hid_root
}

// ── PC Builder ───────────────────────────────────────────────────────────────

/// Build a Property Context inside a HeapBuilder.
pub fn build_pc(heap: &mut HeapBuilder, properties: &[(u16, PropertyValue)]) -> u32 {
    let mut records: Vec<(u16, Vec<u8>)> = Vec::new();

    for (prop_id, value) in properties {
        let record = match value {
            PropertyValue::I32(v) => {
                let mut r = Vec::new();
                r.extend_from_slice(&PTYP_INTEGER_32.to_le_bytes());
                r.extend_from_slice(&v.to_le_bytes());
                r.resize(6, 0);
                r
            }
            PropertyValue::Bool(v) => {
                let mut r = Vec::new();
                r.extend_from_slice(&PTYP_BOOLEAN.to_le_bytes());
                r.extend_from_slice(&(*v as u32).to_le_bytes());
                r.resize(6, 0);
                r
            }
            PropertyValue::I64(v) => {
                let mut r = Vec::new();
                r.extend_from_slice(&PTYP_INTEGER_64.to_le_bytes());
                let hid = heap.alloc(&v.to_le_bytes());
                r.extend_from_slice(&hid.to_le_bytes());
                r
            }
            PropertyValue::Time(v) => {
                let mut r = Vec::new();
                r.extend_from_slice(&PTYP_TIME.to_le_bytes());
                let hid = heap.alloc(&v.to_le_bytes());
                r.extend_from_slice(&hid.to_le_bytes());
                r
            }
            PropertyValue::String(s) => {
                let mut r = Vec::new();
                r.extend_from_slice(&PTYP_STRING.to_le_bytes());
                let utf16: Vec<u8> = s.encode_utf16().flat_map(|c| c.to_le_bytes()).collect();
                let hid = heap.alloc(&utf16);
                r.extend_from_slice(&hid.to_le_bytes());
                r
            }
        };
        records.push((*prop_id, record));
    }

    build_bth(heap, 2, 6, &mut records)
}

#[derive(Debug, Clone)]
pub enum PropertyValue {
    I32(i32),
    Bool(bool),
    I64(i64),
    Time(i64),
    String(String),
}

// ── TC Builder ───────────────────────────────────────────────────────────────

/// Build a Table Context for small inline tables.
pub fn build_tc_inline(
    heap: &mut HeapBuilder,
    columns: &[(u16, u16, u16, u8, u8)], // prop_id, prop_type, ib_data, cb_data, i_bit
    rows: &[Vec<u8>],                    // raw row data
) -> u32 {
    // TCINFO header
    let mut tcinfo = Vec::new();
    tcinfo.push(0x7C); // bType
    tcinfo.push(columns.len() as u8); // cCols

    // rgib[4] — offsets for column groups
    // We use a simple layout: all columns at fixed offsets, total width = last col offset + size
    let _total_width = columns.iter().map(|c| c.3 as u16).max().unwrap_or(0)
        + columns.iter().map(|c| c.2).max().unwrap_or(0);
    // Actually, rgib defines boundaries between groups. For simplicity:
    // rgib[0] = end of 4-byte cols, rgib[1] = end of 8-byte cols,
    // rgib[2] = end of variable cols, rgib[3] = total row width
    let total_row_width = rows.first().map(|r| r.len() as u16).unwrap_or(0);
    tcinfo.extend_from_slice(&0u16.to_le_bytes()); // rgib[0]
    tcinfo.extend_from_slice(&0u16.to_le_bytes()); // rgib[1]
    tcinfo.extend_from_slice(&0u16.to_le_bytes()); // rgib[2]
    tcinfo.extend_from_slice(&total_row_width.to_le_bytes()); // rgib[3]

    tcinfo.extend_from_slice(&0u32.to_le_bytes()); // hidRowIndex placeholder
    tcinfo.extend_from_slice(&0u32.to_le_bytes()); // hnidRows placeholder

    // Column descriptors
    for col in columns {
        tcinfo.extend_from_slice(&col.0.to_le_bytes()); // prop_id
        tcinfo.extend_from_slice(&col.1.to_le_bytes()); // prop_type
        tcinfo.extend_from_slice(&col.2.to_le_bytes()); // ib_data
        tcinfo.push(col.3); // cb_data
        tcinfo.push(col.4); // i_bit
    }

    let hid_tcinfo = heap.alloc(&tcinfo);
    let tcinfo_index = ((hid_tcinfo >> 5) - 1) as usize;

    // Row data
    let mut row_data = Vec::new();
    for row in rows {
        row_data.extend_from_slice(row);
    }
    let hid_rows = heap.alloc(&row_data);

    // Patch hnidRows in TCINFO (at offset 14 within the allocation)
    let tcinfo_start = heap.allocations[tcinfo_index].0;
    heap.data[tcinfo_start + 14..tcinfo_start + 18].copy_from_slice(&hid_rows.to_le_bytes());

    // For now, hidRowIndex is 0 (no row index BTH for small tables)

    hid_tcinfo
}

// ── Main Writer ──────────────────────────────────────────────────────────────

/// Write a complete PST from parsed EML messages.
pub fn write_pst_from_emls<P: AsRef<Path>>(output_path: P, emls: &[eml::EmlMessage]) -> Result<()> {
    let mut layout = Layout::new();

    // ── Build message store PC ───────────────────────────────────────────────
    let store_heap = {
        let mut heap = HeapBuilder::new(0x6C); // bClientSig for PC
        let hid = build_pc(
            &mut heap,
            &[(
                PID_TAG_DISPLAY_NAME,
                PropertyValue::String("Personal Folders".to_string()),
            )],
        );
        heap.finalize(hid)
    };
    layout.add_node(NID_MESSAGE_STORE, store_heap, 0);

    // ── Build named property map (stub) ──────────────────────────────────────
    let named_heap = {
        let mut heap = HeapBuilder::new(0x6C);
        let hid = build_pc(&mut heap, &[]);
        heap.finalize(hid)
    };
    layout.add_node(NID_NAME_TO_ID_MAP, named_heap, 0);

    // ── Build root folder PC ────────────────────────────────────────────────
    let root_folder_heap = {
        let mut heap = HeapBuilder::new(0x6C);
        let hid = build_pc(
            &mut heap,
            &[
                (
                    PID_TAG_DISPLAY_NAME,
                    PropertyValue::String("Root".to_string()),
                ),
                (PID_TAG_CONTENT_COUNT, PropertyValue::I32(0)),
            ],
        );
        heap.finalize(hid)
    };
    layout.add_node(NID_ROOT_FOLDER, root_folder_heap, 0);

    // ── Build root hierarchy TC (1 row: PROMOTIONS folder) ──────────────────
    let promotions_nid = layout.alloc_nid(NID_TYPE_NORMAL_FOLDER);
    let root_hierarchy_heap = {
        let mut heap = HeapBuilder::new(0xBC); // bClientSig for TC
        let columns = vec![(PID_TAG_LTP_ROW_ID, PTYP_INTEGER_32, 0, 4, 0)];
        let rows = vec![promotions_nid.to_le_bytes().to_vec()];
        let hid = build_tc_inline(&mut heap, &columns, &rows);
        heap.finalize(hid)
    };
    layout.add_node((NID_ROOT_FOLDER & !0x1F) | 0x0D, root_hierarchy_heap, 0);

    // ── Build root contents TC (empty) ──────────────────────────────────────
    let root_contents_heap = {
        let mut heap = HeapBuilder::new(0xBC);
        let columns = vec![(PID_TAG_LTP_ROW_ID, PTYP_INTEGER_32, 0, 4, 0)];
        let hid = build_tc_inline(&mut heap, &columns, &[]);
        heap.finalize(hid)
    };
    layout.add_node((NID_ROOT_FOLDER & !0x1F) | 0x0E, root_contents_heap, 0);

    // ── Build PROMOTIONS folder PC ──────────────────────────────────────────
    let promotions_folder_heap = {
        let mut heap = HeapBuilder::new(0x6C);
        let hid = build_pc(
            &mut heap,
            &[
                (
                    PID_TAG_DISPLAY_NAME,
                    PropertyValue::String("PROMOTIONS".to_string()),
                ),
                (PID_TAG_CONTENT_COUNT, PropertyValue::I32(emls.len() as i32)),
            ],
        );
        heap.finalize(hid)
    };
    layout.add_node(promotions_nid, promotions_folder_heap, NID_ROOT_FOLDER);

    // ── Build PROMOTIONS hierarchy TC (empty) ───────────────────────────────
    let promotions_hierarchy_heap = {
        let mut heap = HeapBuilder::new(0xBC);
        let columns = vec![(PID_TAG_LTP_ROW_ID, PTYP_INTEGER_32, 0, 4, 0)];
        let hid = build_tc_inline(&mut heap, &columns, &[]);
        heap.finalize(hid)
    };
    layout.add_node(
        (promotions_nid & !0x1F) | 0x0D,
        promotions_hierarchy_heap,
        0,
    );

    // ── Build PROMOTIONS contents TC (message rows) ─────────────────────────
    let mut message_nids = Vec::new();
    let promotions_contents_rows: Vec<Vec<u8>> = emls
        .iter()
        .map(|_| {
            let nid = layout.alloc_nid(NID_TYPE_NORMAL_MESSAGE);
            message_nids.push(nid);
            nid.to_le_bytes().to_vec()
        })
        .collect();

    let promotions_contents_heap = {
        let mut heap = HeapBuilder::new(0xBC);
        let columns = vec![(PID_TAG_LTP_ROW_ID, PTYP_INTEGER_32, 0, 4, 0)];
        let hid = build_tc_inline(&mut heap, &columns, &promotions_contents_rows);
        heap.finalize(hid)
    };
    layout.add_node((promotions_nid & !0x1F) | 0x0E, promotions_contents_heap, 0);

    // ── Build message PCs ────────────────────────────────────────────────────
    for (i, eml) in emls.iter().enumerate() {
        let msg_nid = message_nids[i];
        let msg_heap = {
            let mut heap = HeapBuilder::new(0x6C);
            let mut props: Vec<(u16, PropertyValue)> = vec![
                (PID_TAG_SUBJECT, PropertyValue::String(eml.subject.clone())),
                (
                    PID_TAG_SENDER_EMAIL_ADDRESS,
                    PropertyValue::String(eml.sender.clone()),
                ),
                (
                    PID_TAG_INTERNET_MESSAGE_ID,
                    PropertyValue::String(eml.message_id.clone()),
                ),
                (
                    PID_TAG_MESSAGE_SIZE,
                    PropertyValue::I32(eml.body.len() as i32),
                ),
                (PID_TAG_HAS_ATTACHMENTS, PropertyValue::Bool(false)),
            ];
            if let Some(ft) = eml.submit_time {
                props.push((PID_TAG_CLIENT_SUBMIT_TIME, PropertyValue::Time(ft)));
            }
            if !eml.body.is_empty() {
                let body_truncated: String = eml.body.chars().take(2000).collect();
                props.push((PID_TAG_BODY, PropertyValue::String(body_truncated)));
            }
            let hid = build_pc(&mut heap, &props);
            heap.finalize(hid)
        };
        layout.add_node(msg_nid, msg_heap, promotions_nid);
    }

    // ── Reserve pages ────────────────────────────────────────────────────────
    // AMap page
    layout.reserve_page(0x84);

    // NBT pages: 1 intermediate + enough leaf pages
    let nbt_leaf_count = layout.nodes.len().div_ceil(16); // 16 entries per page (488/32 ≈ 15)
    let nbt_intermediate_bid = layout.reserve_page(0x81);
    let mut nbt_leaf_bids = Vec::new();
    for _ in 0..nbt_leaf_count {
        nbt_leaf_bids.push(layout.reserve_page(0x81));
    }

    // BBT pages: 1 intermediate + enough leaf pages
    let bbt_leaf_count = layout.blocks.len().div_ceil(20); // 20 entries per page (488/24 ≈ 20)
    let bbt_intermediate_bid = layout.reserve_page(0x80);
    let mut bbt_leaf_bids = Vec::new();
    for _ in 0..bbt_leaf_count {
        bbt_leaf_bids.push(layout.reserve_page(0x80));
    }

    // ── Calculate offsets ────────────────────────────────────────────────────
    layout.calculate_offsets();

    // ── Write file ───────────────────────────────────────────────────────────
    let mut file = File::create(output_path)?;

    // Header
    write_header(&mut file, &layout)?;

    // AMap page
    let amap_page = layout.pages.iter().find(|p| p.ptype == 0x84).unwrap();
    write_amap_page(&mut file, amap_page.bid, &layout)?;

    // NBT intermediate page
    let nbt_intermediate = layout
        .pages
        .iter()
        .find(|p| p.bid == nbt_intermediate_bid)
        .unwrap();
    write_nbt_intermediate(&mut file, nbt_intermediate.bid, &nbt_leaf_bids, &layout)?;

    // NBT leaf pages
    let nbt_leaf_count = nbt_leaf_bids.len();
    for (leaf_index, leaf_bid) in nbt_leaf_bids.iter().enumerate() {
        let page = layout.pages.iter().find(|p| p.bid == *leaf_bid).unwrap();
        write_nbt_leaf_page(
            &mut file,
            page.bid,
            page.offset,
            leaf_index,
            nbt_leaf_count,
            &layout,
        )?;
    }

    // BBT intermediate page
    let bbt_intermediate = layout
        .pages
        .iter()
        .find(|p| p.bid == bbt_intermediate_bid)
        .unwrap();
    write_bbt_intermediate(&mut file, bbt_intermediate.bid, &bbt_leaf_bids, &layout)?;

    // BBT leaf pages
    let bbt_leaf_count = bbt_leaf_bids.len();
    for (leaf_index, leaf_bid) in bbt_leaf_bids.iter().enumerate() {
        let page = layout.pages.iter().find(|p| p.bid == *leaf_bid).unwrap();
        write_bbt_leaf_page(
            &mut file,
            page.bid,
            page.offset,
            leaf_index,
            bbt_leaf_count,
            &layout,
        )?;
    }

    // Data blocks
    for block in &layout.blocks {
        file.seek(SeekFrom::Start(block.offset))?;
        write_data_block(&mut file, block.bid, &block.data)?;
    }

    Ok(())
}

// ── Page Implementation Helpers ──────────────────────────────────────────────

fn write_amap_page<W: Write>(writer: &mut W, page_bid: u64, _layout: &Layout) -> Result<()> {
    let mut page = vec![0u8; PAGE_SIZE as usize];
    // AMap content: 496 bytes of allocation bits
    // Mark everything as allocated (all 1s)
    page[..496].fill(0xFF);

    // Page trailer
    let trailer_offset = PAGE_SIZE as usize - 16;
    page[trailer_offset] = 0x84; // ptype = AMap
    page[trailer_offset + 1] = 0x84;
    page[trailer_offset + 2..trailer_offset + 4].copy_from_slice(&0u16.to_le_bytes());
    let crc = crc32fast::hash(&page[..trailer_offset]);
    page[trailer_offset + 4..trailer_offset + 8].copy_from_slice(&crc.to_le_bytes());
    page[trailer_offset + 8..trailer_offset + 16].copy_from_slice(&page_bid.to_le_bytes());

    writer.write_all(&page)?;
    Ok(())
}

fn write_nbt_intermediate<W: Write>(
    writer: &mut W,
    page_bid: u64,
    child_bids: &[u64],
    layout: &Layout,
) -> Result<()> {
    let mut page = vec![0u8; PAGE_SIZE as usize];

    // Entries: key(8) + BREF(16) = 24 bytes
    let mut offset = 0;
    for child_bid in child_bids {
        if offset + 24 > 488 {
            break;
        }
        // Key: we use the first NID in the child page's range
        // For simplicity, we need to know which nodes go on which page.
        // This requires a more sophisticated layout.
        // For now, let's use a simple sequential split.
        let child_page = layout.pages.iter().find(|p| p.bid == *child_bid).unwrap();
        page[offset..offset + 8].copy_from_slice(&child_page.bid.to_le_bytes());
        page[offset + 8..offset + 16].copy_from_slice(&child_page.bid.to_le_bytes());
        page[offset + 16..offset + 24].copy_from_slice(&child_page.offset.to_le_bytes());
        offset += 24;
    }

    // BTPAGE header
    page[488] = (child_bids.len().min(20)) as u8;
    page[489] = 20;
    page[490] = 8;
    page[491] = 1; // cLevel = intermediate
    page[492..496].fill(0);

    // Page trailer
    let trailer_offset = PAGE_SIZE as usize - 16;
    page[trailer_offset] = 0x81;
    page[trailer_offset + 1] = 0x81;
    page[trailer_offset + 2..trailer_offset + 4].copy_from_slice(&0u16.to_le_bytes());
    let crc = crc32fast::hash(&page[..trailer_offset]);
    page[trailer_offset + 4..trailer_offset + 8].copy_from_slice(&crc.to_le_bytes());
    page[trailer_offset + 8..trailer_offset + 16].copy_from_slice(&page_bid.to_le_bytes());

    writer.write_all(&page)?;
    Ok(())
}

fn write_nbt_leaf_page<W: Write>(
    writer: &mut W,
    page_bid: u64,
    _page_offset: u64,
    leaf_index: usize,
    leaf_count: usize,
    layout: &Layout,
) -> Result<()> {
    let mut page = vec![0u8; PAGE_SIZE as usize];

    // Distribute nodes evenly across leaf pages.
    let nodes_per_page = layout.nodes.len().div_ceil(leaf_count);
    let start = leaf_index * nodes_per_page;
    let end = (start + nodes_per_page).min(layout.nodes.len());

    let mut offset = 0;
    for node in &layout.nodes[start..end] {
        if offset + 32 > 488 {
            break;
        }
        // NBTENTRY: nid(8) + bidData(8) + bidSub(8) + nidParent(4) + dwPadding(4)
        page[offset..offset + 8].copy_from_slice(&node.nid.to_le_bytes());
        page[offset + 8..offset + 16].copy_from_slice(&node.bid_data.to_le_bytes());
        page[offset + 16..offset + 24].copy_from_slice(&node.bid_sub.to_le_bytes());
        page[offset + 24..offset + 28].copy_from_slice(&(node.nid_parent as u32).to_le_bytes());
        page[offset + 28..offset + 32].fill(0);
        offset += 32;
    }

    // BTPAGE header
    let c_entries = (end - start).min(15) as u8;
    page[488] = c_entries;
    page[489] = 15;
    page[490] = 8;
    page[491] = 0;
    page[492..496].fill(0);

    // Page trailer
    let trailer_offset = PAGE_SIZE as usize - 16;
    page[trailer_offset] = 0x81;
    page[trailer_offset + 1] = 0x81;
    page[trailer_offset + 2..trailer_offset + 4].copy_from_slice(&0u16.to_le_bytes());
    let crc = crc32fast::hash(&page[..trailer_offset]);
    page[trailer_offset + 4..trailer_offset + 8].copy_from_slice(&crc.to_le_bytes());
    page[trailer_offset + 8..trailer_offset + 16].copy_from_slice(&page_bid.to_le_bytes());

    writer.write_all(&page)?;
    Ok(())
}

fn write_bbt_intermediate<W: Write>(
    writer: &mut W,
    page_bid: u64,
    child_bids: &[u64],
    layout: &Layout,
) -> Result<()> {
    let mut page = vec![0u8; PAGE_SIZE as usize];

    let mut offset = 0;
    for child_bid in child_bids {
        if offset + 24 > 488 {
            break;
        }
        let child_page = layout.pages.iter().find(|p| p.bid == *child_bid).unwrap();
        page[offset..offset + 8].copy_from_slice(&child_page.bid.to_le_bytes());
        page[offset + 8..offset + 16].copy_from_slice(&child_page.bid.to_le_bytes());
        page[offset + 16..offset + 24].copy_from_slice(&child_page.offset.to_le_bytes());
        offset += 24;
    }

    page[488] = (child_bids.len().min(20)) as u8;
    page[489] = 20;
    page[490] = 8;
    page[491] = 1;
    page[492..496].fill(0);

    let trailer_offset = PAGE_SIZE as usize - 16;
    page[trailer_offset] = 0x80;
    page[trailer_offset + 1] = 0x80;
    page[trailer_offset + 2..trailer_offset + 4].copy_from_slice(&0u16.to_le_bytes());
    let crc = crc32fast::hash(&page[..trailer_offset]);
    page[trailer_offset + 4..trailer_offset + 8].copy_from_slice(&crc.to_le_bytes());
    page[trailer_offset + 8..trailer_offset + 16].copy_from_slice(&page_bid.to_le_bytes());

    writer.write_all(&page)?;
    Ok(())
}

fn write_bbt_leaf_page<W: Write>(
    writer: &mut W,
    page_bid: u64,
    _page_offset: u64,
    leaf_index: usize,
    leaf_count: usize,
    layout: &Layout,
) -> Result<()> {
    let mut page = vec![0u8; PAGE_SIZE as usize];

    let blocks_per_page = layout.blocks.len().div_ceil(leaf_count);
    let start = leaf_index * blocks_per_page;
    let end = (start + blocks_per_page).min(layout.blocks.len());

    let mut offset = 0;
    for block in &layout.blocks[start..end] {
        if offset + 24 > 488 {
            break;
        }
        // BBTENTRY: BREF(16) + cb(2) + cRef(2) + dwPadding(4)
        page[offset..offset + 8].copy_from_slice(&block.bid.to_le_bytes());
        page[offset + 8..offset + 16].copy_from_slice(&block.offset.to_le_bytes());
        page[offset + 16..offset + 18].copy_from_slice(&(block.data.len() as u16).to_le_bytes());
        page[offset + 18..offset + 20].copy_from_slice(&1u16.to_le_bytes());
        page[offset + 20..offset + 24].fill(0);
        offset += 24;
    }

    let c_entries = (end - start).min(20) as u8;
    page[488] = c_entries;
    page[489] = 20;
    page[490] = 8;
    page[491] = 0;
    page[492..496].fill(0);

    let trailer_offset = PAGE_SIZE as usize - 16;
    page[trailer_offset] = 0x80;
    page[trailer_offset + 1] = 0x80;
    page[trailer_offset + 2..trailer_offset + 4].copy_from_slice(&0u16.to_le_bytes());
    let crc = crc32fast::hash(&page[..trailer_offset]);
    page[trailer_offset + 4..trailer_offset + 8].copy_from_slice(&crc.to_le_bytes());
    page[trailer_offset + 8..trailer_offset + 16].copy_from_slice(&page_bid.to_le_bytes());

    writer.write_all(&page)?;
    Ok(())
}
