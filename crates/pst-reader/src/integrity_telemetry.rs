//! Data-path integrity counters for page/block CRC and BID mismatches (track 0077).
//!
//! # Design rules
//!
//! - **CRC stays warning-only and non-fatal.** Counters and rate-limited log lines
//!   never change parse acceptance. A mismatch still returns `Ok` from the NDB
//!   validators; this module only *observes*.
//! - **Count first, log second.** Suppressed emissions are still counted exactly.
//! - **Counters live in the data path**, not in a `tracing` Layer. Totals reach
//!   reports whether or not a subscriber is installed (release Desk installs none).
//! - **Per-thread hot path:** `Cell<u64>` increments; flush into process-global
//!   `AtomicU64` at `snapshot` / `flush_summary` merge points (survives 0079
//!   parallel materialize attribution — see D-0077-parallel-attrib).
//!
//! # Tests
//!
//! Process-global state means telemetry unit tests must not run concurrently with
//! each other. Guard with [`TEST_LOCK`] and call [`reset`] under that lock.

use std::cell::Cell;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Cap for distinct bad BIDs tracked (per-thread and global merge).
pub const DISTINCT_BAD_BIDS_CAP: usize = 1024;

/// Default first-N detail lines per category before aggregation.
pub const DEFAULT_FIRST_N: u64 = 10;

/// Default minimum interval between aggregate summary lines.
pub const DEFAULT_SUMMARY_INTERVAL: Duration = Duration::from_secs(30);

/// Serialize telemetry tests that touch process-global counters.
pub static TEST_LOCK: Mutex<()> = Mutex::new(());

// ─── Public snapshot ─────────────────────────────────────────────────────────

/// Point-in-time integrity counters (process-wide after flush).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IntegritySnapshot {
    pub page_crc_mismatches: u64,
    pub block_crc_mismatches: u64,
    pub block_bid_mismatches: u64,
    pub distinct_bad_bids: u64,
    pub distinct_bad_bids_exact: bool,
    pub page_reads: u64,
    pub block_reads: u64,
}

impl IntegritySnapshot {
    /// Counter-wise subtraction (saturating). Used for per-source attribution.
    pub fn delta_since(&self, prev: &IntegritySnapshot) -> IntegritySnapshot {
        IntegritySnapshot {
            page_crc_mismatches: self
                .page_crc_mismatches
                .saturating_sub(prev.page_crc_mismatches),
            block_crc_mismatches: self
                .block_crc_mismatches
                .saturating_sub(prev.block_crc_mismatches),
            block_bid_mismatches: self
                .block_bid_mismatches
                .saturating_sub(prev.block_bid_mismatches),
            // Prefer [`end_source_delta`] for per-source distinct BIDs.
            // This subtractive path keeps global post-state only when activity
            // occurred (legacy callers); sequential multi-source scans should
            // use begin_source/end_source_delta instead.
            distinct_bad_bids: if self.page_crc_mismatches > prev.page_crc_mismatches
                || self.block_crc_mismatches > prev.block_crc_mismatches
                || self.block_bid_mismatches > prev.block_bid_mismatches
            {
                self.distinct_bad_bids
            } else {
                0
            },
            distinct_bad_bids_exact: self.distinct_bad_bids_exact,
            page_reads: self.page_reads.saturating_sub(prev.page_reads),
            block_reads: self.block_reads.saturating_sub(prev.block_reads),
        }
    }

    /// Sum of CRC/BID mismatch counters.
    pub fn mismatch_total(&self) -> u64 {
        self.page_crc_mismatches
            .saturating_add(self.block_crc_mismatches)
            .saturating_add(self.block_bid_mismatches)
    }
}

// ─── Config / globals ────────────────────────────────────────────────────────

struct LogConfig {
    first_n: u64,
    summary_interval: Duration,
}

static LOG_CONFIG: Mutex<LogConfig> = Mutex::new(LogConfig {
    first_n: DEFAULT_FIRST_N,
    summary_interval: DEFAULT_SUMMARY_INTERVAL,
});

static G_PAGE_CRC: AtomicU64 = AtomicU64::new(0);
static G_BLOCK_CRC: AtomicU64 = AtomicU64::new(0);
static G_BLOCK_BID: AtomicU64 = AtomicU64::new(0);
static G_PAGE_READS: AtomicU64 = AtomicU64::new(0);
static G_BLOCK_READS: AtomicU64 = AtomicU64::new(0);
static G_BAD_BIDS_EXACT: AtomicBool = AtomicBool::new(true);
static G_EMISSIONS: AtomicU64 = AtomicU64::new(0);

struct GlobalBadBids {
    bids: HashSet<u64>,
    exact: bool,
}

impl GlobalBadBids {
    fn new() -> Self {
        Self {
            bids: HashSet::new(),
            exact: true,
        }
    }
}

/// Lazily initialized so we can construct a `HashSet` without const-new.
fn global_bad_bids() -> &'static Mutex<GlobalBadBids> {
    static CELL: std::sync::OnceLock<Mutex<GlobalBadBids>> = std::sync::OnceLock::new();
    CELL.get_or_init(|| Mutex::new(GlobalBadBids::new()))
}

// ─── Thread-local hot path ───────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MismatchKind {
    PageCrc,
    BlockCrc,
}

struct TlsState {
    page_crc: Cell<u64>,
    block_crc: Cell<u64>,
    block_bid: Cell<u64>,
    page_reads: Cell<u64>,
    block_reads: Cell<u64>,
    /// Detail lines already emitted per category (for first-N gate).
    page_crc_emitted: Cell<u64>,
    block_crc_emitted: Cell<u64>,
    block_bid_emitted: Cell<u64>,
    last_aggregate: Cell<Option<Instant>>,
    /// Per-thread distinct BIDs (capped) — merged into process-global on flush.
    bad_bids: std::cell::RefCell<HashSet<u64>>,
    bad_bids_exact: Cell<bool>,
    /// Per-source distinct BIDs (capped) — not drained on flush; cleared by
    /// [`begin_source`]. Powers source-local `distinct_bad_bids` attribution.
    source_bad_bids: std::cell::RefCell<HashSet<u64>>,
    source_bad_bids_exact: Cell<bool>,
}

impl TlsState {
    fn new() -> Self {
        Self {
            page_crc: Cell::new(0),
            block_crc: Cell::new(0),
            block_bid: Cell::new(0),
            page_reads: Cell::new(0),
            block_reads: Cell::new(0),
            page_crc_emitted: Cell::new(0),
            block_crc_emitted: Cell::new(0),
            block_bid_emitted: Cell::new(0),
            last_aggregate: Cell::new(None),
            bad_bids: std::cell::RefCell::new(HashSet::new()),
            bad_bids_exact: Cell::new(true),
            source_bad_bids: std::cell::RefCell::new(HashSet::new()),
            source_bad_bids_exact: Cell::new(true),
        }
    }

    fn mismatch_total(&self) -> u64 {
        self.page_crc
            .get()
            .saturating_add(self.block_crc.get())
            .saturating_add(self.block_bid.get())
    }
}

thread_local! {
    static TLS: TlsState = TlsState::new();
}

// ─── Public API ──────────────────────────────────────────────────────────────

/// Configure emission gate. `first_n = u64::MAX` restores pre-0077 firehose.
/// `first_n = 0` emits totals-only (aggregates + flush).
pub fn set_log_limit(first_n: u64, summary_interval: Duration) {
    if let Ok(mut cfg) = LOG_CONFIG.lock() {
        cfg.first_n = first_n;
        cfg.summary_interval = summary_interval;
    }
}

/// Flush thread-local counters into process globals and return a snapshot.
pub fn snapshot() -> IntegritySnapshot {
    flush_tls_to_global();
    read_global_snapshot()
}

/// Begin attribution for one source PST (0077 per-source distinct BIDs).
///
/// Flushes TLS counters into globals, clears the **source-local** distinct-BID
/// set (process-global totals are retained), and returns a global snapshot for
/// use with [`end_source_delta`].
pub fn begin_source() -> IntegritySnapshot {
    flush_tls_to_global();
    TLS.with(|t| {
        t.source_bad_bids.borrow_mut().clear();
        t.source_bad_bids_exact.set(true);
    });
    read_global_snapshot()
}

/// End-of-source delta: counter-wise subtraction vs `before` plus **source-local**
/// distinct BID cardinality (not the process-global set size).
///
/// Call after the source walk (and after [`flush_summary`] if desired). Does not
/// clear the source set; the next [`begin_source`] does.
pub fn end_source_delta(before: &IntegritySnapshot) -> IntegritySnapshot {
    flush_tls_to_global();
    let after = read_global_snapshot();
    let (source_distinct, source_exact) = TLS.with(|t| {
        (
            t.source_bad_bids.borrow().len() as u64,
            t.source_bad_bids_exact.get(),
        )
    });
    IntegritySnapshot {
        page_crc_mismatches: after
            .page_crc_mismatches
            .saturating_sub(before.page_crc_mismatches),
        block_crc_mismatches: after
            .block_crc_mismatches
            .saturating_sub(before.block_crc_mismatches),
        block_bid_mismatches: after
            .block_bid_mismatches
            .saturating_sub(before.block_bid_mismatches),
        distinct_bad_bids: source_distinct,
        distinct_bad_bids_exact: source_exact,
        page_reads: after.page_reads.saturating_sub(before.page_reads),
        block_reads: after.block_reads.saturating_sub(before.block_reads),
    }
}

/// Emit a final aggregate line (end-of-source / end-of-run) and flush TLS.
pub fn flush_summary() {
    flush_tls_to_global();
    let snap = read_global_snapshot();
    if snap.mismatch_total() == 0 {
        return;
    }
    tracing::warn!(
        page_crc = snap.page_crc_mismatches,
        block_crc = snap.block_crc_mismatches,
        block_bid = snap.block_bid_mismatches,
        distinct_bad_bids = snap.distinct_bad_bids,
        distinct_bad_bids_exact = snap.distinct_bad_bids_exact,
        page_reads = snap.page_reads,
        block_reads = snap.block_reads,
        "integrity_telemetry flush_summary"
    );
    G_EMISSIONS.fetch_add(1, Ordering::Relaxed);
}

/// Reset all process-global and current-thread counters (tests only).
pub fn reset() {
    G_PAGE_CRC.store(0, Ordering::Relaxed);
    G_BLOCK_CRC.store(0, Ordering::Relaxed);
    G_BLOCK_BID.store(0, Ordering::Relaxed);
    G_PAGE_READS.store(0, Ordering::Relaxed);
    G_BLOCK_READS.store(0, Ordering::Relaxed);
    G_BAD_BIDS_EXACT.store(true, Ordering::Relaxed);
    G_EMISSIONS.store(0, Ordering::Relaxed);
    if let Ok(mut g) = global_bad_bids().lock() {
        g.bids.clear();
        g.exact = true;
    }
    if let Ok(mut cfg) = LOG_CONFIG.lock() {
        cfg.first_n = DEFAULT_FIRST_N;
        cfg.summary_interval = DEFAULT_SUMMARY_INTERVAL;
    }
    TLS.with(|t| {
        t.page_crc.set(0);
        t.block_crc.set(0);
        t.block_bid.set(0);
        t.page_reads.set(0);
        t.block_reads.set(0);
        t.page_crc_emitted.set(0);
        t.block_crc_emitted.set(0);
        t.block_bid_emitted.set(0);
        t.last_aggregate.set(None);
        t.bad_bids.borrow_mut().clear();
        t.bad_bids_exact.set(true);
        t.source_bad_bids.borrow_mut().clear();
        t.source_bad_bids_exact.set(true);
    });
}

/// Emission count (detail + aggregate + flush lines). Test surface.
pub fn emissions_count() -> u64 {
    G_EMISSIONS.load(Ordering::Relaxed)
}

/// Thread-local mismatch total (page CRC + block CRC + block BID). No flush.
pub fn tls_mismatch_total() -> u64 {
    TLS.with(|t| t.mismatch_total())
}

/// Thread-local **block-level** mismatches only (block CRC + BID).
///
/// Message-scope `CRC_SUSPECT` taint uses this rather than full mismatch total:
/// some real PSTs / fixtures use a non-standard *page* CRC polynomial (see
/// `ndb/page.rs`), so page CRC warnings are still counted for the report but
/// must not taint every message as data corruption (DoD-12 clean fixtures).
pub fn tls_block_mismatch_total() -> u64 {
    TLS.with(|t| t.block_crc.get().saturating_add(t.block_bid.get()))
}

/// Token for message-scope CRC taint (enter/exit delta on TLS **block** mismatches).
///
/// Nested scopes are safe: each compares against its own start total. Page CRC
/// is **not** included (poly-class fixtures); only block CRC + BID count.
#[derive(Debug)]
pub struct MessageScope {
    start: u64,
}

/// Snapshot TLS block-mismatch total on entry to a message/attachment read.
pub fn message_scope_enter() -> MessageScope {
    MessageScope {
        start: tls_block_mismatch_total(),
    }
}

impl MessageScope {
    /// True when any block CRC or BID mismatch was counted during the scope.
    pub fn exit(self) -> bool {
        tls_block_mismatch_total() > self.start
    }
}

/// Run `f` under a message CRC scope; OR any block CRC/BID delta into `tainted`.
///
/// Prefer this for attach-meta / attach-stream paths that must contribute to
/// the same message's `CRC_SUSPECT` after properties already returned.
pub fn with_crc_scope<R>(tainted: &mut bool, f: impl FnOnce() -> R) -> R {
    let scope = message_scope_enter();
    let result = f();
    if scope.exit() {
        *tainted = true;
    }
    result
}

/// Count a page validate path (denominator for `block_crc_read_rate`).
pub fn note_page_read() {
    TLS.with(|t| t.page_reads.set(t.page_reads.get().saturating_add(1)));
}

/// Count a block read path (denominator for `block_crc_read_rate`).
pub fn note_block_read() {
    TLS.with(|t| t.block_reads.set(t.block_reads.get().saturating_add(1)));
}

/// Record a page CRC mismatch (warning-only; still counted).
pub fn note_page_crc(bid: u64, computed: u32, stored: u32) {
    note_mismatch(MismatchKind::PageCrc, bid, computed, stored);
}

/// Record a block CRC mismatch (warning-only; still counted).
pub fn note_block_crc(bid: u64, computed: u32, stored: u32) {
    note_mismatch(MismatchKind::BlockCrc, bid, computed, stored);
}

/// Record a block BID mismatch (warning-only; still counted).
pub fn note_block_bid_mismatch(ib: u64, bbt_bid: u64, trailer_bid: u64) {
    TLS.with(|t| {
        t.block_bid.set(t.block_bid.get().saturating_add(1));
        record_bad_bid(t, bbt_bid);
        if trailer_bid != bbt_bid {
            record_bad_bid(t, trailer_bid);
        }
        maybe_emit_bid(t, ib, bbt_bid, trailer_bid);
    });
}

// ─── Internals ───────────────────────────────────────────────────────────────

fn note_mismatch(kind: MismatchKind, bid: u64, computed: u32, stored: u32) {
    TLS.with(|t| {
        match kind {
            MismatchKind::PageCrc => t.page_crc.set(t.page_crc.get().saturating_add(1)),
            MismatchKind::BlockCrc => t.block_crc.set(t.block_crc.get().saturating_add(1)),
        }
        record_bad_bid(t, bid);
        maybe_emit(t, kind, bid, computed, stored);
    });
}

fn record_bad_bid(t: &TlsState, bid: u64) {
    // Merge set (flushed into process-global).
    {
        let mut set = t.bad_bids.borrow_mut();
        if set.len() >= DISTINCT_BAD_BIDS_CAP {
            if !set.contains(&bid) {
                t.bad_bids_exact.set(false);
            }
        } else {
            set.insert(bid);
        }
    }
    // Source-local set (for per-file distinct_bad_bids attribution).
    {
        let mut set = t.source_bad_bids.borrow_mut();
        if set.len() >= DISTINCT_BAD_BIDS_CAP {
            if !set.contains(&bid) {
                t.source_bad_bids_exact.set(false);
            }
        } else {
            set.insert(bid);
        }
    }
}

fn read_log_config() -> (u64, Duration) {
    LOG_CONFIG
        .lock()
        .map(|c| (c.first_n, c.summary_interval))
        .unwrap_or((DEFAULT_FIRST_N, DEFAULT_SUMMARY_INTERVAL))
}

fn maybe_emit(t: &TlsState, kind: MismatchKind, bid: u64, computed: u32, stored: u32) {
    let (first_n, interval) = read_log_config();

    let emitted_cell = match kind {
        MismatchKind::PageCrc => &t.page_crc_emitted,
        MismatchKind::BlockCrc => &t.block_crc_emitted,
    };
    let already = emitted_cell.get();

    if already < first_n {
        emitted_cell.set(already.saturating_add(1));
        match kind {
            MismatchKind::PageCrc => {
                tracing::warn!(
                    "Page CRC mismatch at bid=0x{bid:016X}: computed={computed:08X}, stored={stored:08X}"
                );
            }
            MismatchKind::BlockCrc => {
                tracing::warn!(
                    "Block CRC mismatch at bid=0x{bid:016X}: computed={computed:08X}, stored={stored:08X}"
                );
            }
        }
        G_EMISSIONS.fetch_add(1, Ordering::Relaxed);
        return;
    }

    maybe_aggregate(t, interval);
}

fn maybe_emit_bid(t: &TlsState, ib: u64, bbt_bid: u64, trailer_bid: u64) {
    let (first_n, interval) = read_log_config();
    let already = t.block_bid_emitted.get();
    if already < first_n {
        t.block_bid_emitted.set(already.saturating_add(1));
        tracing::warn!(
            "Block BID mismatch at ib=0x{ib:X}: BBT says 0x{bbt_bid:016X}, trailer says 0x{trailer_bid:016X}"
        );
        G_EMISSIONS.fetch_add(1, Ordering::Relaxed);
        return;
    }
    maybe_aggregate(t, interval);
}

fn maybe_aggregate(t: &TlsState, interval: Duration) {
    // Aggregate path: at most one line per interval (per thread).
    let now = Instant::now();
    let last = t.last_aggregate.get();
    let due = match last {
        None => true,
        Some(prev) => now.duration_since(prev) >= interval,
    };
    if due {
        t.last_aggregate.set(Some(now));
        tracing::warn!(
            page_crc = t.page_crc.get(),
            block_crc = t.block_crc.get(),
            block_bid = t.block_bid.get(),
            "integrity_telemetry aggregate (running thread-local totals)"
        );
        G_EMISSIONS.fetch_add(1, Ordering::Relaxed);
    }
}

fn flush_tls_to_global() {
    TLS.with(|t| {
        let page_crc = t.page_crc.replace(0);
        let block_crc = t.block_crc.replace(0);
        let block_bid = t.block_bid.replace(0);
        let page_reads = t.page_reads.replace(0);
        let block_reads = t.block_reads.replace(0);
        if page_crc > 0 {
            G_PAGE_CRC.fetch_add(page_crc, Ordering::Relaxed);
        }
        if block_crc > 0 {
            G_BLOCK_CRC.fetch_add(block_crc, Ordering::Relaxed);
        }
        if block_bid > 0 {
            G_BLOCK_BID.fetch_add(block_bid, Ordering::Relaxed);
        }
        if page_reads > 0 {
            G_PAGE_READS.fetch_add(page_reads, Ordering::Relaxed);
        }
        if block_reads > 0 {
            G_BLOCK_READS.fetch_add(block_reads, Ordering::Relaxed);
        }

        let mut local = t.bad_bids.borrow_mut();
        let local_exact = t.bad_bids_exact.get();
        if let Ok(mut g) = global_bad_bids().lock() {
            for bid in local.drain() {
                if g.bids.len() >= DISTINCT_BAD_BIDS_CAP {
                    if !g.bids.contains(&bid) {
                        g.exact = false;
                    }
                } else {
                    g.bids.insert(bid);
                }
            }
            if !local_exact {
                g.exact = false;
            }
            G_BAD_BIDS_EXACT.store(g.exact, Ordering::Relaxed);
        }
        t.bad_bids_exact.set(true);
    });
}

fn read_global_snapshot() -> IntegritySnapshot {
    let (distinct, exact) = global_bad_bids()
        .lock()
        .map(|g| (g.bids.len() as u64, g.exact))
        .unwrap_or((0, true));
    IntegritySnapshot {
        page_crc_mismatches: G_PAGE_CRC.load(Ordering::Relaxed),
        block_crc_mismatches: G_BLOCK_CRC.load(Ordering::Relaxed),
        block_bid_mismatches: G_BLOCK_BID.load(Ordering::Relaxed),
        distinct_bad_bids: distinct,
        distinct_bad_bids_exact: exact && G_BAD_BIDS_EXACT.load(Ordering::Relaxed),
        page_reads: G_PAGE_READS.load(Ordering::Relaxed),
        block_reads: G_BLOCK_READS.load(Ordering::Relaxed),
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn with_lock<F: FnOnce()>(f: F) {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        reset();
        f();
        reset();
    }

    #[test]
    fn counts_and_delta() {
        with_lock(|| {
            note_page_read();
            note_page_crc(0xA, 1, 2);
            note_block_read();
            note_block_crc(0xB, 3, 4);
            note_block_bid_mismatch(0x100, 0xC, 0xD);

            let s = snapshot();
            assert_eq!(s.page_crc_mismatches, 1);
            assert_eq!(s.block_crc_mismatches, 1);
            assert_eq!(s.block_bid_mismatches, 1);
            assert_eq!(s.page_reads, 1);
            assert_eq!(s.block_reads, 1);
            assert!(s.distinct_bad_bids >= 2);
            assert!(s.distinct_bad_bids_exact);

            let mid = snapshot();
            note_page_crc(0xE, 5, 6);
            let end = snapshot();
            let d = end.delta_since(&mid);
            assert_eq!(d.page_crc_mismatches, 1);
            assert_eq!(d.block_crc_mismatches, 0);
        });
    }

    #[test]
    fn source_local_distinct_bad_bids() {
        with_lock(|| {
            // Source A: two distinct bad BIDs.
            let a0 = begin_source();
            note_block_crc(0x11, 1, 2);
            note_block_crc(0x22, 3, 4);
            let a = end_source_delta(&a0);
            assert_eq!(a.block_crc_mismatches, 2);
            assert_eq!(a.distinct_bad_bids, 2);

            // Source B: one new BID — must report 1, not cumulative 3.
            let b0 = begin_source();
            note_block_crc(0x33, 5, 6);
            let b = end_source_delta(&b0);
            assert_eq!(b.block_crc_mismatches, 1);
            assert_eq!(
                b.distinct_bad_bids, 1,
                "per-source distinct must not include prior source BIDs"
            );

            // Global still accumulates.
            let g = snapshot();
            assert_eq!(g.block_crc_mismatches, 3);
            assert_eq!(g.distinct_bad_bids, 3);
        });
    }

    #[test]
    fn distinct_cap_marks_inexact() {
        with_lock(|| {
            for i in 0..(DISTINCT_BAD_BIDS_CAP as u64 + 50) {
                note_block_crc(i, 1, 2);
            }
            let s = snapshot();
            assert_eq!(s.block_crc_mismatches, DISTINCT_BAD_BIDS_CAP as u64 + 50);
            assert_eq!(s.distinct_bad_bids, DISTINCT_BAD_BIDS_CAP as u64);
            assert!(!s.distinct_bad_bids_exact);
        });
    }

    #[test]
    fn reset_clears() {
        with_lock(|| {
            note_page_crc(1, 1, 2);
            let _ = snapshot();
            reset();
            let s = snapshot();
            assert_eq!(s.page_crc_mismatches, 0);
            assert_eq!(s.distinct_bad_bids, 0);
            assert!(s.distinct_bad_bids_exact);
        });
    }

    #[test]
    fn bounded_emission_with_exact_total() {
        with_lock(|| {
            set_log_limit(10, Duration::from_secs(3600));
            // Measure delta so parallel fixture tests that hit real CRC cannot
            // pollute the assertion (process-global emission counter).
            let base = emissions_count();
            const N: u64 = 10_000;
            for i in 0..N {
                note_page_crc(i % 100, 1, 2);
            }
            let s = snapshot();
            assert_eq!(s.page_crc_mismatches, N);
            // first_n detail + at most one aggregate in the long interval window
            // before flush; flush_summary adds one more.
            let before_flush = emissions_count().saturating_sub(base);
            // Bound is first_n detail + sparse aggregates ≪ N. Allow a little headroom
            // for Instant granularity across hosts; still proves rate-limit vs firehose.
            assert!(
                before_flush < 50,
                "emissions before flush should be bounded, got {before_flush}"
            );
            flush_summary();
            let total_lines = emissions_count().saturating_sub(base);
            assert!(
                total_lines < 55,
                "emissions including flush should be bounded, got {total_lines}"
            );
            assert!(total_lines >= 10, "first_n detail lines expected");
        });
    }

    #[test]
    fn message_scope_detects_delta() {
        with_lock(|| {
            let scope = message_scope_enter();
            assert!(!scope.exit());
            let scope = message_scope_enter();
            note_block_crc(9, 1, 2);
            assert!(scope.exit());
            // Page CRC alone does not taint message scope (poly false-positive class).
            let scope = message_scope_enter();
            note_page_crc(1, 1, 2);
            assert!(!scope.exit());
        });
    }

    #[test]
    fn firehose_first_n_max() {
        with_lock(|| {
            set_log_limit(u64::MAX, Duration::from_secs(3600));
            for i in 0..50 {
                note_block_crc(i, 1, 2);
            }
            assert_eq!(emissions_count(), 50);
        });
    }
}
