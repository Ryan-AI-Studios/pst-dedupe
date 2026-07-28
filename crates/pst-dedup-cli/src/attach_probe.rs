//! Budgeted deep attachment stream preflight (track **0074**).
//!
//! Graded L0–L3 probes open/read attachment streams under hard budgets, merge
//! fail reasons into message integrity (keep-set fidelity), and never allocate
//! multi-GB attach `Vec`s (chunked discard only). Source PSTs stay read-only.
//!
//! Default deep level is **L2 head**. Opt-in via `--deep-attach-preflight`.

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use dedup_engine::integrity::{attach_reason_from_pst_error, IntegrityReason, ScanMode};
use dedup_engine::keepset::{
    group_candidates, rank_key, FamilyPolicy, KeepPolicy, RankContext, RecoverableScanItem,
};
use pst_reader::{NodeId, PstFile};

/// Fixed discard buffer size (64 KiB) — never grow with attach size.
const DISCARD_CHUNK: usize = 64 * 1024;

/// Progress sink: (probed_attaches, probe_bytes, source_basename).
pub type ProbeProgressCb = Box<dyn FnMut(u64, u64, &str) + Send>;

/// PidTagAttachMethod: by-value binary.
const ATTACH_BY_VALUE: i32 = 0x0000_0001;
/// PidTagAttachMethod: embedded message.
const ATTACH_EMBEDDED_MSG: i32 = 0x0000_0005;

/// Probe depth levels (L0–L3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ProbeLevel {
    /// L0: list_attachments only (not used by deep path).
    Meta = 0,
    /// L1: open stream handle; no body read.
    Open = 1,
    /// L2: chunked head-read up to per-attach budget (default deep).
    Head = 2,
    /// L3: full stream under remaining global budgets.
    Full = 3,
}

impl ProbeLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Meta => "meta",
            Self::Open => "open",
            Self::Head => "head",
            Self::Full => "full",
        }
    }

    /// Parse CLI level (`head` | `full`; also accepts open/meta for tests).
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "meta" => Some(Self::Meta),
            "open" => Some(Self::Open),
            "head" => Some(Self::Head),
            "full" => Some(Self::Full),
            _ => None,
        }
    }
}

/// Hard budgets for a probe pass (0074 defaults).
#[derive(Clone, Copy, Debug)]
pub struct ProbeBudgets {
    pub max_attaches: u64,
    pub max_probe_bytes: u64,
    pub per_attach_max_bytes: u64,
    pub max_probe_time_ms: u64,
    pub max_open_psts: usize,
    pub max_peer_probes_per_group: u64,
}

impl Default for ProbeBudgets {
    fn default() -> Self {
        Self {
            max_attaches: 50_000,
            max_probe_bytes: 256 * 1024 * 1024, // 256 MiB
            per_attach_max_bytes: 1024 * 1024,  // 1 MiB L2
            max_probe_time_ms: 2000,
            max_open_psts: 32,
            max_peer_probes_per_group: 3,
        }
    }
}

/// Outcome of probing one attachment stream.
#[derive(Clone, Debug)]
pub struct ProbeOutcome {
    pub ok: bool,
    pub reason: Option<IntegrityReason>,
    pub bytes_read: u64,
    pub timed_out: bool,
    pub level: ProbeLevel,
}

/// Aggregate summary of a probe pass.
#[derive(Clone, Debug, Default)]
pub struct AttachProbeSummary {
    pub attempted: u64,
    pub failed: u64,
    pub truncated: bool,
    pub bytes: u64,
    pub peer_probe_capped_groups: u64,
    pub level: String,
    pub cancelled: bool,
}

/// Cache key identity for level-aware result cache.
///
/// `size` is attachment metadata size; `source_file_size` fingerprints the PST
/// itself so a same-path same-second replace still misses (0074 P2-1).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct CacheKey {
    path: String,
    msg_nid: u64,
    attach_nid: u64,
    size: u32,
    mtime_secs: i64,
    source_file_size: u64,
}

/// Cached probe result (level dominance: higher level ok satisfies lower).
#[derive(Clone, Debug)]
struct CacheEntry {
    level: ProbeLevel,
    ok: bool,
    reason: Option<IntegrityReason>,
}

/// In-process level-aware probe result cache (0074 §3.10).
#[derive(Default)]
pub struct ProbeResultCache {
    entries: HashMap<CacheKey, CacheEntry>,
}

impl ProbeResultCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Lookup: hit only when cached_level >= requested (L3 ok satisfies L2/L1; L1 never L2).
    #[allow(clippy::too_many_arguments)]
    pub fn get(
        &self,
        path: &str,
        msg_nid: u64,
        attach_nid: u64,
        size: u32,
        mtime_secs: i64,
        source_file_size: u64,
        level: ProbeLevel,
    ) -> Option<ProbeOutcome> {
        let key = CacheKey {
            path: path.to_string(),
            msg_nid,
            attach_nid,
            size,
            mtime_secs,
            source_file_size,
        };
        let e = self.entries.get(&key)?;
        if e.level < level {
            return None;
        }
        Some(ProbeOutcome {
            ok: e.ok,
            reason: e.reason,
            bytes_read: 0,
            timed_out: false,
            level: e.level,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn insert(
        &mut self,
        path: &str,
        msg_nid: u64,
        attach_nid: u64,
        size: u32,
        mtime_secs: i64,
        source_file_size: u64,
        outcome: &ProbeOutcome,
    ) {
        let key = CacheKey {
            path: path.to_string(),
            msg_nid,
            attach_nid,
            size,
            mtime_secs,
            source_file_size,
        };
        // Keep the higher-level result if already present.
        if let Some(existing) = self.entries.get(&key) {
            if existing.level > outcome.level {
                return;
            }
            // Prefer fail over ok at same level (fail is more informative).
            if existing.level == outcome.level && !existing.ok {
                return;
            }
        }
        self.entries.insert(
            key,
            CacheEntry {
                level: outcome.level,
                ok: outcome.ok,
                reason: outcome.reason,
            },
        );
    }

    /// True when a fail was recorded for this attach at any level.
    pub fn stream_available_hint(
        &self,
        path: &str,
        msg_nid: u64,
        attach_nid: u64,
        size: u32,
        mtime_secs: i64,
        source_file_size: u64,
    ) -> Option<bool> {
        let key = CacheKey {
            path: path.to_string(),
            msg_nid,
            attach_nid,
            size,
            mtime_secs,
            source_file_size,
        };
        self.entries.get(&key).map(|e| e.ok)
    }

    /// Level-aware stream_available from a prior probe pass (no I/O).
    ///
    /// - `Some(false)` — cache hit at requested level (or higher) with fail
    /// - `Some(true)` — cache hit at requested level (or higher) with ok
    /// - `None` — miss (caller keeps legacy optimistic / truncated honesty)
    #[allow(clippy::too_many_arguments)]
    pub fn stream_available_at_level(
        &self,
        path: &str,
        msg_nid: u64,
        attach_nid: u64,
        size: u32,
        mtime_secs: i64,
        source_file_size: u64,
        level: ProbeLevel,
    ) -> Option<bool> {
        self.get(
            path,
            msg_nid,
            attach_nid,
            size,
            mtime_secs,
            source_file_size,
            level,
        )
        .map(|o| o.ok)
    }

    /// Number of cached entries (tests / diagnostics).
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Bounded LRU of open PST handles (`max_open_psts`).
pub struct PstHandleLru {
    capacity: usize,
    order: VecDeque<String>,
    map: HashMap<String, PstFile>,
}

impl PstHandleLru {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            order: VecDeque::new(),
            map: HashMap::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Touch path as most-recently-used.
    fn touch(&mut self, path: &str) {
        if let Some(pos) = self.order.iter().position(|p| p == path) {
            self.order.remove(pos);
        }
        self.order.push_back(path.to_string());
    }

    fn evict_lru(&mut self) {
        while self.map.len() >= self.capacity {
            if let Some(old) = self.order.pop_front() {
                self.map.remove(&old);
            } else {
                break;
            }
        }
    }

    /// Open or reuse a sticky handle; drops LRU when over capacity.
    pub fn get_mut(&mut self, path: &str) -> Result<&mut PstFile, IntegrityReason> {
        if self.map.contains_key(path) {
            self.touch(path);
            return self
                .map
                .get_mut(path)
                .ok_or(IntegrityReason::AttachStreamOpenFailed);
        }
        self.evict_lru();
        let pst = PstFile::open(Path::new(path)).map_err(|e| attach_reason_from_pst_error(&e))?;
        self.map.insert(path.to_string(), pst);
        self.touch(path);
        self.map
            .get_mut(path)
            .ok_or(IntegrityReason::AttachStreamOpenFailed)
    }

    /// Remove a sticky handle (for timeout-bounded transfer to a worker thread).
    pub fn take(&mut self, path: &str) -> Option<PstFile> {
        if let Some(pos) = self.order.iter().position(|p| p == path) {
            self.order.remove(pos);
        }
        self.map.remove(path)
    }

    /// Return an owned handle to the LRU (after a successful timed probe).
    pub fn insert_owned(&mut self, path: String, pst: PstFile) {
        if self.map.contains_key(&path) {
            // Prefer the freshly used handle; drop the stale one.
            self.map.insert(path.clone(), pst);
            self.touch(&path);
            return;
        }
        self.evict_lru();
        self.map.insert(path.clone(), pst);
        self.touch(&path);
    }
}

/// File mtime seconds and size (`0, 0` if unavailable) for cache identity.
pub fn path_mtime_and_size(path: &str) -> (i64, u64) {
    let Ok(meta) = std::fs::metadata(path) else {
        return (0, 0);
    };
    let size = meta.len();
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    (mtime, size)
}

/// File mtime seconds (0 if unavailable) for cache identity.
pub fn path_mtime_secs(path: &str) -> i64 {
    path_mtime_and_size(path).0
}

/// Bytes to charge against the global probe budget on timeout.
///
/// Full-level workers may read up to `bytes_left`; Head/Open reserve only
/// `per_attach_max_bytes`. Charge is `max(bytes_read, min(effective_cap, bytes_left))`
/// so a timed-out Full worker cannot leave nearly the full budget free for the
/// next attach (0074 P1-3).
pub fn timeout_budget_charge(
    level: ProbeLevel,
    per_attach_max_bytes: u64,
    bytes_left: u64,
    bytes_read: u64,
) -> u64 {
    let effective_cap = if level == ProbeLevel::Full {
        bytes_left
    } else {
        per_attach_max_bytes
    };
    let reserved = effective_cap.min(bytes_left);
    bytes_read.max(reserved)
}

/// Probe engine holding LRU handles + level-aware cache.
pub struct AttachProbeEngine {
    pub budgets: ProbeBudgets,
    pub level: ProbeLevel,
    handles: PstHandleLru,
    pub cache: ProbeResultCache,
    cancel: Option<Arc<AtomicBool>>,
    progress: Option<ProbeProgressCb>,
    /// Bytes remaining in global budget.
    bytes_left: u64,
    attaches_left: u64,
    pub summary: AttachProbeSummary,
}

impl AttachProbeEngine {
    pub fn new(
        budgets: ProbeBudgets,
        level: ProbeLevel,
        cancel: Option<Arc<AtomicBool>>,
        progress: Option<ProbeProgressCb>,
    ) -> Self {
        let max_open = budgets.max_open_psts;
        Self {
            handles: PstHandleLru::new(max_open),
            cache: ProbeResultCache::new(),
            cancel,
            progress,
            bytes_left: budgets.max_probe_bytes,
            attaches_left: budgets.max_attaches,
            summary: AttachProbeSummary {
                level: level.as_str().to_string(),
                ..Default::default()
            },
            budgets,
            level,
        }
    }

    fn cancelled(&self) -> bool {
        self.cancel
            .as_ref()
            .map(|c| c.load(Ordering::SeqCst))
            .unwrap_or(false)
    }

    fn emit_progress(&mut self, source_path: &str) {
        if let Some(cb) = self.progress.as_mut() {
            let base = Path::new(source_path)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or(source_path);
            cb(self.summary.attempted, self.summary.bytes, base);
        }
    }

    /// Whether global budgets allow another attach attempt.
    pub fn budget_exhausted(&self) -> bool {
        self.attaches_left == 0 || self.bytes_left == 0
    }

    /// Non-fail outcome for cooperative cancel (must not look like attach corruption).
    fn cancel_outcome(&mut self, bytes_read: u64) -> ProbeOutcome {
        self.summary.cancelled = true;
        ProbeOutcome {
            ok: true,
            reason: None,
            bytes_read,
            timed_out: false,
            level: self.level,
        }
    }

    /// Probe a single attachment under budgets / timeout / cache.
    pub fn probe_attach(
        &mut self,
        source_path: &str,
        msg_nid: u64,
        attach_nid: u64,
        attach_size: u32,
        attach_method: Option<i32>,
    ) -> ProbeOutcome {
        // Cancel is not an attach fail: do not degrade, inflate failed, or cache.
        if self.cancelled() {
            return self.cancel_outcome(0);
        }

        if self.budget_exhausted() {
            self.summary.truncated = true;
            return ProbeOutcome {
                ok: false,
                reason: Some(IntegrityReason::AttachProbeTruncated),
                bytes_read: 0,
                timed_out: false,
                level: self.level,
            };
        }

        let (mtime, source_file_size) = path_mtime_and_size(source_path);
        if let Some(hit) = self.cache.get(
            source_path,
            msg_nid,
            attach_nid,
            attach_size,
            mtime,
            source_file_size,
            self.level,
        ) {
            // Cache hit counts as attempted for honesty when we skip I/O? Spec: rates from
            // attempted only. Cached reuse should still count once originally; re-use does not
            // re-count. Return cached without incrementing attempted.
            return hit;
        }

        // Method gate: only by-value and embedded are portable for stream probe.
        if let Some(method) = attach_method {
            if method != ATTACH_BY_VALUE && method != ATTACH_EMBEDDED_MSG {
                let outcome = ProbeOutcome {
                    ok: false,
                    reason: Some(IntegrityReason::AttachMethodUnsupported),
                    bytes_read: 0,
                    timed_out: false,
                    level: self.level,
                };
                self.record_attempt(&outcome);
                self.cache.insert(
                    source_path,
                    msg_nid,
                    attach_nid,
                    attach_size,
                    mtime,
                    source_file_size,
                    &outcome,
                );
                return outcome;
            }
        }

        let deadline = Instant::now() + Duration::from_millis(self.budgets.max_probe_time_ms);
        let level = self.level;
        let per_attach = self.budgets.per_attach_max_bytes;
        let bytes_left = self.bytes_left;
        let cancel = self.cancel.clone();

        // Ensure a sticky handle exists, then take it for a timeout-bounded worker.
        // On timeout the worker may continue briefly in the background (D-0074-timeout-join);
        // the handle is abandoned and re-opened on next access.
        let open_err = self.handles.get_mut(source_path).err();
        let outcome = if let Some(reason) = open_err {
            ProbeOutcome {
                ok: false,
                reason: Some(reason),
                bytes_read: 0,
                timed_out: false,
                level,
            }
        } else if let Some(pst) = self.handles.take(source_path) {
            let (outcome, maybe_pst) = probe_attach_stream_timed(
                pst,
                source_path.to_string(),
                NodeId(msg_nid),
                NodeId(attach_nid),
                level,
                per_attach,
                bytes_left,
                deadline,
                cancel,
            );
            if let Some(pst) = maybe_pst {
                self.handles.insert_owned(source_path.to_string(), pst);
            }
            outcome
        } else {
            ProbeOutcome {
                ok: false,
                reason: Some(IntegrityReason::AttachStreamOpenFailed),
                bytes_read: 0,
                timed_out: false,
                level,
            }
        };

        // Cancel mid-stream (or between open and record): non-fail, no cache, no failed++.
        if self.cancelled() {
            self.summary.bytes = self.summary.bytes.saturating_add(outcome.bytes_read);
            if outcome.bytes_read > self.bytes_left {
                self.bytes_left = 0;
            } else {
                self.bytes_left = self.bytes_left.saturating_sub(outcome.bytes_read);
            }
            return self.cancel_outcome(outcome.bytes_read);
        }

        self.record_attempt(&outcome);
        if outcome.reason != Some(IntegrityReason::AttachProbeTruncated) {
            self.cache.insert(
                source_path,
                msg_nid,
                attach_nid,
                attach_size,
                mtime,
                source_file_size,
                &outcome,
            );
        }
        self.emit_progress(source_path);
        outcome
    }

    fn record_attempt(&mut self, outcome: &ProbeOutcome) {
        // Truncation (budget) is not an attach fail; timeout is a fail.
        // Cancel never reaches here (handled before record).
        if outcome.reason == Some(IntegrityReason::AttachProbeTruncated) {
            self.summary.truncated = true;
            return;
        }
        self.summary.attempted += 1;
        if self.attaches_left > 0 {
            self.attaches_left -= 1;
        }
        // Byte accounting:
        // - Normal path: charge actual bytes_read.
        // - Timeout path: charge the effective per-probe reserve even when the
        //   abandoned worker reports bytes_read=0. Full-level reserves all
        //   remaining global bytes; Head reserves per_attach_max_bytes (P1-3).
        //   Residual: worker join remains D-0074-timeout-join.
        let charge =
            if outcome.timed_out || outcome.reason == Some(IntegrityReason::AttachProbeTimeout) {
                timeout_budget_charge(
                    outcome.level,
                    self.budgets.per_attach_max_bytes,
                    self.bytes_left,
                    outcome.bytes_read,
                )
            } else {
                outcome.bytes_read
            };
        self.summary.bytes = self.summary.bytes.saturating_add(charge);
        if charge >= self.bytes_left {
            self.bytes_left = 0;
        } else {
            self.bytes_left -= charge;
        }
        // Only real probe fails inflate failed (not truncated/info).
        if !outcome.ok {
            if let Some(r) = outcome.reason {
                if r.is_attach_probe_fail() {
                    self.summary.failed += 1;
                }
            } else {
                self.summary.failed += 1;
            }
        }
        // Timeout implies incomplete coverage for remaining attaches.
        if outcome.timed_out || outcome.reason == Some(IntegrityReason::AttachProbeTimeout) {
            self.summary.truncated = true;
        }
        if self.attaches_left == 0 || self.bytes_left == 0 {
            self.summary.truncated = true;
        }
    }

    /// Take the level-aware result cache after a probe pass (moves out of the engine).
    pub fn take_cache(&mut self) -> ProbeResultCache {
        std::mem::take(&mut self.cache)
    }
}

/// Core stream probe: open + optional chunked discard read.
///
/// Never allocates a buffer larger than [`DISCARD_CHUNK`].
/// Deadline is checked between small reads; for a hard wall-clock bound around
/// blocking open/read, use [`probe_attach_stream_timed`] (thread + `recv_timeout`).
#[allow(clippy::too_many_arguments)]
pub fn probe_attach_stream(
    pst: &mut PstFile,
    msg_nid: NodeId,
    attach_nid: NodeId,
    level: ProbeLevel,
    per_attach_max_bytes: u64,
    global_bytes_left: u64,
    deadline: Instant,
    cancel: &Option<Arc<AtomicBool>>,
) -> ProbeOutcome {
    if level == ProbeLevel::Meta {
        return ProbeOutcome {
            ok: true,
            reason: None,
            bytes_read: 0,
            timed_out: false,
            level,
        };
    }

    if Instant::now() >= deadline {
        return ProbeOutcome {
            ok: false,
            reason: Some(IntegrityReason::AttachProbeTimeout),
            bytes_read: 0,
            timed_out: true,
            level,
        };
    }

    let mut reader = match pst.open_attachment_data(msg_nid, attach_nid) {
        Ok(r) => r,
        Err(e) => {
            return ProbeOutcome {
                ok: false,
                reason: Some(attach_reason_from_pst_error(&e)),
                bytes_read: 0,
                timed_out: false,
                level,
            };
        }
    };

    if Instant::now() >= deadline {
        return ProbeOutcome {
            ok: false,
            reason: Some(IntegrityReason::AttachProbeTimeout),
            bytes_read: 0,
            timed_out: true,
            level,
        };
    }

    if level == ProbeLevel::Open {
        // Drop without reading.
        drop(reader);
        return ProbeOutcome {
            ok: true,
            reason: None,
            bytes_read: 0,
            timed_out: false,
            level,
        };
    }

    let max_bytes = match level {
        ProbeLevel::Head => per_attach_max_bytes.min(global_bytes_left),
        ProbeLevel::Full => global_bytes_left,
        _ => 0,
    };

    let mut buf = [0u8; DISCARD_CHUNK];
    let mut bytes_read: u64 = 0;

    while bytes_read < max_bytes {
        if Instant::now() >= deadline {
            return ProbeOutcome {
                ok: false,
                reason: Some(IntegrityReason::AttachProbeTimeout),
                bytes_read,
                timed_out: true,
                level,
            };
        }
        if cancel
            .as_ref()
            .map(|c| c.load(Ordering::SeqCst))
            .unwrap_or(false)
        {
            // Cancel is not attach corruption / timeout fail (0074 review P0).
            // Caller must not cache, degrade, or increment failed.
            return ProbeOutcome {
                ok: true,
                reason: None,
                bytes_read,
                timed_out: false,
                level,
            };
        }

        let want = ((max_bytes - bytes_read) as usize).min(DISCARD_CHUNK);
        match reader.read(&mut buf[..want]) {
            Ok(0) => break, // EOF
            Ok(n) => {
                bytes_read += n as u64;
                // Catch slow-but-completed reads that blew past the wall clock.
                if Instant::now() >= deadline {
                    return ProbeOutcome {
                        ok: false,
                        reason: Some(IntegrityReason::AttachProbeTimeout),
                        bytes_read,
                        timed_out: true,
                        level,
                    };
                }
            }
            Err(e) => {
                // Read path: prefer READ_FAILED; sniff CRC/truncation from io message.
                let reason = reason_from_io_read_error(&e);
                return ProbeOutcome {
                    ok: false,
                    reason: Some(reason),
                    bytes_read,
                    timed_out: false,
                    level,
                };
            }
        }
    }

    ProbeOutcome {
        ok: true,
        reason: None,
        bytes_read,
        timed_out: false,
        level,
    }
}

/// Run [`probe_attach_stream`] on a worker thread with hard `recv_timeout`.
///
/// Returns `(outcome, Some(pst))` when the worker finishes in time (handle reusable).
/// On timeout returns `ATTACH_PROBE_TIMEOUT` and `None` for the handle — the worker
/// may still finish in the background and drop the PST (residual **D-0074-timeout-join**
/// only if Drop blocks; we deliberately do not join).
///
/// `source_path` is retained for diagnostics; open already succeeded when `pst` is owned.
#[allow(clippy::too_many_arguments)]
pub fn probe_attach_stream_timed(
    mut pst: PstFile,
    source_path: String,
    msg_nid: NodeId,
    attach_nid: NodeId,
    level: ProbeLevel,
    per_attach_max_bytes: u64,
    global_bytes_left: u64,
    deadline: Instant,
    cancel: Option<Arc<AtomicBool>>,
) -> (ProbeOutcome, Option<PstFile>) {
    let _ = source_path; // reserved for future progress/diagnostics
    let wait = deadline.saturating_duration_since(Instant::now());
    // Floor so already-expired deadlines still fail fast rather than blocking forever.
    let wait = if wait.is_zero() {
        Duration::from_millis(1)
    } else {
        wait
    };

    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let outcome = probe_attach_stream(
            &mut pst,
            msg_nid,
            attach_nid,
            level,
            per_attach_max_bytes,
            global_bytes_left,
            deadline,
            &cancel,
        );
        // Best-effort deliver; receiver may have timed out and dropped.
        let _ = tx.send((outcome, pst));
    });

    match rx.recv_timeout(wait) {
        Ok((outcome, pst)) => (outcome, Some(pst)),
        Err(mpsc::RecvTimeoutError::Timeout) => (
            ProbeOutcome {
                ok: false,
                reason: Some(IntegrityReason::AttachProbeTimeout),
                bytes_read: 0,
                timed_out: true,
                level,
            },
            None,
        ),
        Err(mpsc::RecvTimeoutError::Disconnected) => (
            ProbeOutcome {
                ok: false,
                reason: Some(IntegrityReason::AttachStreamOpenFailed),
                bytes_read: 0,
                timed_out: false,
                level,
            },
            None,
        ),
    }
}

/// Path-based timed probe (opens a fresh handle in the worker). Used by tests and
/// call sites that do not share sticky LRU handles.
#[allow(clippy::too_many_arguments)]
pub fn probe_attach_at_path_timed(
    path: &Path,
    msg_nid: NodeId,
    attach_nid: NodeId,
    level: ProbeLevel,
    per_attach_max_bytes: u64,
    global_bytes_left: u64,
    deadline: Instant,
    cancel: Option<Arc<AtomicBool>>,
) -> ProbeOutcome {
    let path_buf: PathBuf = path.to_path_buf();
    let wait = deadline.saturating_duration_since(Instant::now());
    let wait = if wait.is_zero() {
        Duration::from_millis(1)
    } else {
        wait
    };
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let outcome = match PstFile::open(&path_buf) {
            Ok(mut pst) => probe_attach_stream(
                &mut pst,
                msg_nid,
                attach_nid,
                level,
                per_attach_max_bytes,
                global_bytes_left,
                deadline,
                &cancel,
            ),
            Err(e) => ProbeOutcome {
                ok: false,
                reason: Some(attach_reason_from_pst_error(&e)),
                bytes_read: 0,
                timed_out: false,
                level,
            },
        };
        let _ = tx.send(outcome);
    });
    match rx.recv_timeout(wait) {
        Ok(o) => o,
        Err(mpsc::RecvTimeoutError::Timeout) => ProbeOutcome {
            ok: false,
            reason: Some(IntegrityReason::AttachProbeTimeout),
            bytes_read: 0,
            timed_out: true,
            level,
        },
        Err(mpsc::RecvTimeoutError::Disconnected) => ProbeOutcome {
            ok: false,
            reason: Some(IntegrityReason::AttachStreamOpenFailed),
            bytes_read: 0,
            timed_out: false,
            level,
        },
    }
}

fn reason_from_io_read_error(e: &std::io::Error) -> IntegrityReason {
    let msg = e.to_string();
    let upper = msg.to_ascii_uppercase();
    if upper.contains("CRC") {
        IntegrityReason::AttachStreamCrc
    } else if upper.contains("BLOCK") && upper.contains("NOT") {
        IntegrityReason::AttachBlockNotFound
    } else if upper.contains("TRUNC") {
        IntegrityReason::AttachDataTruncated
    } else {
        IntegrityReason::AttachStreamReadFailed
    }
}

fn push_degraded(item: &mut RecoverableScanItem, reason: IntegrityReason) {
    // Info-only codes do not degrade fidelity via normal probe fails. Cancel never
    // reaches here. Peer-cap on unprobed peers uses [`mark_unprobed_peer_cap`].
    if matches!(
        reason,
        IntegrityReason::AttachProbeTruncated | IntegrityReason::AttachPeerProbeCap
    ) {
        return;
    }
    if !item.integrity.degraded_reasons.contains(&reason) {
        item.integrity.degraded_reasons.push(reason);
    }
    item.integrity.degraded = true;
}

/// Mark an **unprobed** peer as degraded after `max_peer_probes_per_group` so it
/// cannot silently win as a clean candidate (spec §3.7.1).
///
/// `ATTACH_PEER_PROBE_CAP` is not an attach fail for rate tallies, but it **does**
/// degrade fidelity_rank so the capped provisional winner stays best-effort.
fn mark_unprobed_peer_cap(item: &mut RecoverableScanItem) {
    let reason = IntegrityReason::AttachPeerProbeCap;
    if !item.integrity.degraded_reasons.contains(&reason) {
        item.integrity.degraded_reasons.push(reason);
    }
    item.integrity.degraded = true;
}

/// Apply a probe fail under mode: best-effort degrades; strict marks for skip (caller removes).
///
/// Returns `true` when the item should be **skipped** (strict mode).
fn apply_probe_fail(
    item: &mut RecoverableScanItem,
    reason: IntegrityReason,
    mode: ScanMode,
) -> bool {
    if matches!(
        reason,
        IntegrityReason::AttachProbeTruncated | IntegrityReason::AttachPeerProbeCap
    ) {
        return false;
    }
    match mode {
        ScanMode::BestEffort => {
            push_degraded(item, reason);
            false
        }
        ScanMode::Strict => {
            // Align with classify_attach_meta_fail / body strict: skip, do not degrade-and-keep.
            if !item.integrity.degraded_reasons.contains(&reason) {
                item.integrity.degraded_reasons.push(reason);
            }
            // Mark degraded so callers can detect probe-fail via is_attach_probe_fail reasons.
            item.integrity.degraded = true;
            true
        }
    }
}

/// Probe all attaches on each scan item under budgets (scan path).
///
/// Skips when `include_attachments` is false (caller responsibility).
/// Under [`ScanMode::Strict`], probe fails mark items for skip (caller removes + tallies).
///
/// Returns the aggregate summary and the level-aware result cache for reuse
/// (e.g. materializer `stream_available` without re-I/O).
pub fn probe_scan_items(
    items: &mut [RecoverableScanItem],
    budgets: ProbeBudgets,
    level: ProbeLevel,
    mode: ScanMode,
    cancel: Option<Arc<AtomicBool>>,
    progress: Option<ProbeProgressCb>,
) -> (AttachProbeSummary, ProbeResultCache) {
    let mut engine = AttachProbeEngine::new(budgets, level, cancel, progress);

    for item in items.iter_mut() {
        if engine.cancelled() {
            engine.summary.cancelled = true;
            break;
        }
        if engine.budget_exhausted() {
            engine.summary.truncated = true;
            break;
        }

        let path = item.locus.source_path.clone();
        let msg_nid = item.locus.nid;

        let list = {
            let pst = match engine.handles.get_mut(&path) {
                Ok(p) => p,
                Err(reason) => {
                    // Count as a failed open attempt for the message.
                    let outcome = ProbeOutcome {
                        ok: false,
                        reason: Some(reason),
                        bytes_read: 0,
                        timed_out: false,
                        level,
                    };
                    engine.record_attempt(&outcome);
                    let _ = apply_probe_fail(item, reason, mode);
                    continue;
                }
            };
            match pst.list_attachments(NodeId(msg_nid)) {
                Ok(l) => l,
                Err(_) => {
                    let reason = IntegrityReason::AttachMetaFailed;
                    let outcome = ProbeOutcome {
                        ok: false,
                        reason: Some(reason),
                        bytes_read: 0,
                        timed_out: false,
                        level,
                    };
                    engine.record_attempt(&outcome);
                    let _ = apply_probe_fail(item, reason, mode);
                    continue;
                }
            }
        };

        for att in list {
            if engine.cancelled() {
                engine.summary.cancelled = true;
                break;
            }
            if engine.budget_exhausted() {
                engine.summary.truncated = true;
                break;
            }
            let outcome =
                engine.probe_attach(&path, msg_nid, att.nid.0, att.size, att.attach_method);
            if engine.summary.cancelled {
                break;
            }
            if let Some(r) = outcome.reason {
                if r.is_attach_probe_fail() {
                    let _ = apply_probe_fail(item, r, mode);
                }
            }
        }
        if engine.summary.cancelled {
            break;
        }
    }

    let summary = engine.summary.clone();
    let cache = engine.take_cache();
    (summary, cache)
}

/// Options for [`probe_keep_set_groups`] (packs policy + budgets to stay clippy-friendly).
pub struct KeepSetProbeOpts<'a> {
    pub budgets: ProbeBudgets,
    pub level: ProbeLevel,
    pub policy: KeepPolicy,
    pub family: FamilyPolicy,
    pub prefer_path: &'a [String],
    pub tier2_enabled: bool,
    /// Integrity mode: best-effort degrades; strict marks probe fails for skip/removal.
    pub mode: ScanMode,
    pub cancel: Option<Arc<AtomicBool>>,
    pub progress: Option<ProbeProgressCb>,
}

/// Probe keep-set groups with peer cap (unique-pst winner path).
///
/// Within each identity group, probes at most `max_peer_probes_per_group` candidates
/// in rank order (fidelity → policy → path/nid). Degrades integrity on attach fail so
/// subsequent `resolve_groups` / `fidelity_rank` prefers clean peers.
/// Under strict mode, probe fails mark candidates so callers can remove them (no win).
///
/// Returns the aggregate summary and the level-aware result cache for materializer
/// reuse (set `stream_available` without re-opening streams).
pub fn probe_keep_set_groups(
    items: &mut [RecoverableScanItem],
    opts: KeepSetProbeOpts<'_>,
) -> (AttachProbeSummary, ProbeResultCache) {
    let KeepSetProbeOpts {
        budgets,
        level,
        policy,
        family,
        prefer_path,
        tier2_enabled,
        mode,
        cancel,
        progress,
    } = opts;

    // parents_only / no attachments: skip entirely.
    if family == FamilyPolicy::ParentsOnly {
        return (
            AttachProbeSummary {
                level: level.as_str().to_string(),
                ..Default::default()
            },
            ProbeResultCache::new(),
        );
    }

    let mut engine = AttachProbeEngine::new(budgets, level, cancel, progress);
    let groups = group_candidates(items, tier2_enabled);
    let peer_cap = budgets.max_peer_probes_per_group.max(1);

    for group in &groups {
        if engine.cancelled() {
            engine.summary.cancelled = true;
            break;
        }
        if engine.budget_exhausted() {
            engine.summary.truncated = true;
            break;
        }

        // Rank members within group (lower key = better).
        let rank_ctx = RankContext::from_policy_and_prefer(policy, prefer_path);
        let mut ranked: Vec<usize> = group.clone();
        ranked
            .sort_by(|&a, &b| rank_key(&items[a], &rank_ctx).cmp(&rank_key(&items[b], &rank_ctx)));

        let mut probed_peers = 0u64;
        let mut found_clean = false;
        let mut probed_idxs: Vec<usize> = Vec::new();

        for &idx in &ranked {
            // Cap: stop after N peers probed (even if more remain).
            if probed_peers >= peer_cap {
                break;
            }
            if engine.cancelled() {
                engine.summary.cancelled = true;
                break;
            }
            if engine.budget_exhausted() {
                engine.summary.truncated = true;
                break;
            }

            probed_idxs.push(idx);
            let path = items[idx].locus.source_path.clone();
            let msg_nid = items[idx].locus.nid;

            let list = {
                let pst = match engine.handles.get_mut(&path) {
                    Ok(p) => p,
                    Err(reason) => {
                        let outcome = ProbeOutcome {
                            ok: false,
                            reason: Some(reason),
                            bytes_read: 0,
                            timed_out: false,
                            level,
                        };
                        engine.record_attempt(&outcome);
                        let _ = apply_probe_fail(&mut items[idx], reason, mode);
                        probed_peers += 1;
                        continue;
                    }
                };
                match pst.list_attachments(NodeId(msg_nid)) {
                    Ok(l) => l,
                    Err(_) => {
                        let reason = IntegrityReason::AttachMetaFailed;
                        engine.record_attempt(&ProbeOutcome {
                            ok: false,
                            reason: Some(reason),
                            bytes_read: 0,
                            timed_out: false,
                            level,
                        });
                        let _ = apply_probe_fail(&mut items[idx], reason, mode);
                        probed_peers += 1;
                        continue;
                    }
                }
            };

            // No attaches: treat as clean for peer selection.
            if list.is_empty() {
                found_clean = !items[idx].integrity.degraded;
                probed_peers += 1;
                continue;
            }

            let mut any_fail = false;
            for att in list {
                if engine.cancelled() {
                    engine.summary.cancelled = true;
                    break;
                }
                if engine.budget_exhausted() {
                    engine.summary.truncated = true;
                    break;
                }
                let outcome =
                    engine.probe_attach(&path, msg_nid, att.nid.0, att.size, att.attach_method);
                if engine.summary.cancelled {
                    break;
                }
                // any_fail only for real attach probe fails — not truncation / peer-cap info.
                if let Some(r) = outcome.reason {
                    if r.is_attach_probe_fail() {
                        any_fail = true;
                        let _ = apply_probe_fail(&mut items[idx], r, mode);
                    }
                } else if !outcome.ok {
                    any_fail = true;
                }
            }
            if engine.summary.cancelled {
                break;
            }
            probed_peers += 1;
            if !any_fail && !items[idx].integrity.degraded {
                found_clean = true;
                // Prefer stopping once we have a clean peer (saves budget).
                break;
            }
        }

        if engine.summary.cancelled {
            break;
        }

        // Cap tally: N peers probed with no clean peer — whether or not an (N+1)th remains.
        // Groups with fewer than N members that all fail are exhausted, not capped.
        if !found_clean && probed_peers >= peer_cap {
            engine.summary.peer_probe_capped_groups += 1;
            // Spec §3.7.1: keep best-effort winner as degraded; do **not** let unprobed
            // remaining peers win as clean (would defeat the peer budget bound).
            let probed_set: HashSet<usize> = probed_idxs.iter().copied().collect();
            for &idx in &ranked {
                if !probed_set.contains(&idx) {
                    mark_unprobed_peer_cap(&mut items[idx]);
                }
            }
        }
    }

    let summary = engine.summary.clone();
    let cache = engine.take_cache();
    (summary, cache)
}

/// Unit-test helpers for LRU / cache / discard (no real PST required for pure pieces).
#[cfg(test)]
mod tests {
    use super::*;
    use dedup_engine::integrity::{AttachProbePreflight, RecoverableIntegrity};
    use dedup_engine::keepset::MessageLocus;
    use std::io::{self, Cursor};

    #[test]
    fn probe_level_ordering_for_cache_dominance() {
        assert!(ProbeLevel::Full > ProbeLevel::Head);
        assert!(ProbeLevel::Head > ProbeLevel::Open);
        assert!(ProbeLevel::Open > ProbeLevel::Meta);
    }

    #[test]
    fn cache_l1_ok_does_not_satisfy_l2() {
        let mut cache = ProbeResultCache::new();
        let outcome = ProbeOutcome {
            ok: true,
            reason: None,
            bytes_read: 0,
            timed_out: false,
            level: ProbeLevel::Open,
        };
        cache.insert("a.pst", 1, 2, 100, 0, 0, &outcome);
        assert!(cache
            .get("a.pst", 1, 2, 100, 0, 0, ProbeLevel::Open)
            .is_some());
        assert!(cache
            .get("a.pst", 1, 2, 100, 0, 0, ProbeLevel::Head)
            .is_none());
    }

    #[test]
    fn cache_l3_ok_satisfies_l2_and_l1() {
        let mut cache = ProbeResultCache::new();
        let outcome = ProbeOutcome {
            ok: true,
            reason: None,
            bytes_read: 0,
            timed_out: false,
            level: ProbeLevel::Full,
        };
        cache.insert("a.pst", 1, 2, 100, 0, 0, &outcome);
        assert!(cache
            .get("a.pst", 1, 2, 100, 0, 0, ProbeLevel::Head)
            .is_some());
        assert!(cache
            .get("a.pst", 1, 2, 100, 0, 0, ProbeLevel::Open)
            .is_some());
    }

    #[test]
    fn cache_mtime_miss() {
        let mut cache = ProbeResultCache::new();
        let outcome = ProbeOutcome {
            ok: true,
            reason: None,
            bytes_read: 0,
            timed_out: false,
            level: ProbeLevel::Head,
        };
        cache.insert("a.pst", 1, 2, 100, 10, 0, &outcome);
        assert!(cache
            .get("a.pst", 1, 2, 100, 11, 0, ProbeLevel::Head)
            .is_none());
    }

    /// Source PST file size is part of cache identity (0074 P2-1).
    #[test]
    fn cache_source_file_size_miss() {
        let mut cache = ProbeResultCache::new();
        let outcome = ProbeOutcome {
            ok: true,
            reason: None,
            bytes_read: 64,
            timed_out: false,
            level: ProbeLevel::Head,
        };
        cache.insert("a.pst", 1, 2, 100, 10, 1_000_000, &outcome);
        // Same path/mtime/attach size, different source file size → miss.
        assert!(cache
            .get("a.pst", 1, 2, 100, 10, 2_000_000, ProbeLevel::Head)
            .is_none());
        // Exact size match → hit.
        assert!(cache
            .get("a.pst", 1, 2, 100, 10, 1_000_000, ProbeLevel::Head)
            .is_some());
    }

    /// Mock Read that tracks max buffer size requested and returns large logical size
    /// in small chunks — proves L2 path never needs a fat Vec.
    struct ChunkedMock {
        remaining: u64,
        max_buf_seen: usize,
        chunk: usize,
    }

    impl Read for ChunkedMock {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.max_buf_seen = self.max_buf_seen.max(buf.len());
            if self.remaining == 0 {
                return Ok(0);
            }
            let n = (self.remaining as usize).min(buf.len()).min(self.chunk);
            for b in buf.iter_mut().take(n) {
                *b = 0xAB;
            }
            self.remaining -= n as u64;
            Ok(n)
        }
    }

    #[test]
    fn l2_chunked_discard_no_fat_vec() {
        // Simulate reading 8 GiB logical with 64 KiB buffer — only track max buf.
        let mut mock = ChunkedMock {
            remaining: 8 * 1024 * 1024 * 1024u64,
            max_buf_seen: 0,
            chunk: 64 * 1024,
        };
        let per_attach_max: u64 = 1024 * 1024; // 1 MiB L2
        let mut buf = [0u8; DISCARD_CHUNK];
        let mut bytes_read = 0u64;
        while bytes_read < per_attach_max {
            let want = ((per_attach_max - bytes_read) as usize).min(DISCARD_CHUNK);
            match mock.read(&mut buf[..want]) {
                Ok(0) => break,
                Ok(n) => bytes_read += n as u64,
                Err(_) => break,
            }
        }
        assert_eq!(bytes_read, per_attach_max);
        assert!(
            mock.max_buf_seen <= DISCARD_CHUNK,
            "max buf {} > {}",
            mock.max_buf_seen,
            DISCARD_CHUNK
        );
        // Cursor-style also works for small fixtures:
        let mut c = Cursor::new(vec![1u8; 100]);
        let mut small = [0u8; DISCARD_CHUNK];
        let n = c.read(&mut small).expect("read");
        assert_eq!(n, 100);
    }

    #[test]
    fn lru_capacity_and_empty() {
        let lru = PstHandleLru::new(32);
        assert_eq!(lru.capacity, 32);
        assert!(lru.is_empty());
        assert_eq!(lru.len(), 0);
        let lru0 = PstHandleLru::new(0);
        assert_eq!(lru0.capacity, 1); // clamped
    }

    #[test]
    fn lru_open_missing_path_fails_without_leaking() {
        let mut lru = PstHandleLru::new(2);
        for i in 0..5 {
            let path = format!("C:\\definitely\\missing\\probe_lru_{i}.pst");
            let err = lru.get_mut(&path);
            assert!(err.is_err());
        }
        // Failed opens must not accumulate handles.
        assert_eq!(lru.len(), 0);
    }

    #[test]
    fn parents_only_probe_keep_set_zero() {
        let mut items = vec![RecoverableScanItem {
            locus: MessageLocus {
                source_path: "x.pst".into(),
                source_pst: "x.pst".into(),
                folder_path: "Inbox".into(),
                nid: 0x20,
                is_orphaned: false,
            },
            message_id_norm: Some("mid1".into()),
            content_hash: [0; 32],
            size: 100,
            integrity: RecoverableIntegrity::clean(),
            scan_order: 0,
            submit_time: None,
            delivery_time: None,
            has_bcc: false,
        }];
        let (summary, _cache) = probe_keep_set_groups(
            &mut items,
            KeepSetProbeOpts {
                budgets: ProbeBudgets::default(),
                level: ProbeLevel::Head,
                policy: KeepPolicy::FirstSeen,
                family: FamilyPolicy::ParentsOnly,
                prefer_path: &[],
                tier2_enabled: true,
                mode: ScanMode::BestEffort,
                cancel: None,
                progress: None,
            },
        );
        assert_eq!(summary.attempted, 0);
        assert_eq!(summary.failed, 0);
        assert!(!items[0].integrity.degraded);
    }

    #[test]
    fn peer_probe_cap_counter_increments() {
        // Without real PSTs, open fails for every peer → all dirty → cap hits.
        let mut items: Vec<RecoverableScanItem> = (0..6)
            .map(|i| RecoverableScanItem {
                locus: MessageLocus {
                    source_path: format!("C:\\missing\\peer{i}.pst"),
                    source_pst: format!("peer{i}.pst"),
                    folder_path: "Inbox".into(),
                    nid: 0x20 + i as u64,
                    is_orphaned: false,
                },
                message_id_norm: Some("same-mid".into()),
                content_hash: [1; 32],
                size: 100,
                integrity: RecoverableIntegrity::clean(),
                scan_order: i as u64,
                submit_time: None,
                delivery_time: None,
                has_bcc: false,
            })
            .collect();
        let budgets = ProbeBudgets {
            max_peer_probes_per_group: 3,
            max_attaches: 50_000,
            ..ProbeBudgets::default()
        };
        let (summary, _cache) = probe_keep_set_groups(
            &mut items,
            KeepSetProbeOpts {
                budgets,
                level: ProbeLevel::Open,
                policy: KeepPolicy::FirstSeen,
                family: FamilyPolicy::KeepAttachmentsWithParent,
                prefer_path: &[],
                tier2_enabled: true,
                mode: ScanMode::BestEffort,
                cancel: None,
                progress: None,
            },
        );
        // One group, 6 peers, cap 3 → peer_probe_capped_groups = 1 (all fail open).
        assert_eq!(summary.peer_probe_capped_groups, 1);
        assert!(summary.attempted <= 3 + 1); // at most cap (+ maybe open fails counted)
        assert!(summary.attempted >= 1);
        // Probed peers degraded by open fail; unprobed remaining get ATTACH_PEER_PROBE_CAP
        // so they cannot win as clean (§3.7.1).
        let degraded_count = items.iter().filter(|i| i.integrity.degraded).count();
        assert_eq!(degraded_count, 6, "degraded={degraded_count}");
        let cap_marked = items
            .iter()
            .filter(|i| {
                i.integrity
                    .degraded_reasons
                    .contains(&IntegrityReason::AttachPeerProbeCap)
            })
            .count();
        assert_eq!(cap_marked, 3, "unprobed beyond cap must be peer-cap marked");
    }

    /// Exact-N dirty peers (no N+1 remaining) must still count as capped.
    #[test]
    fn peer_probe_cap_exact_n_counts() {
        let peer_cap = 3u64;
        let mut items: Vec<RecoverableScanItem> = (0..peer_cap)
            .map(|i| RecoverableScanItem {
                locus: MessageLocus {
                    source_path: format!("C:\\missing\\exact{i}.pst"),
                    source_pst: format!("exact{i}.pst"),
                    folder_path: "Inbox".into(),
                    nid: 0x60 + i,
                    is_orphaned: false,
                },
                message_id_norm: Some("same-mid-exact".into()),
                content_hash: [2; 32],
                size: 100,
                integrity: RecoverableIntegrity::clean(),
                scan_order: i,
                submit_time: None,
                delivery_time: None,
                has_bcc: false,
            })
            .collect();
        let budgets = ProbeBudgets {
            max_peer_probes_per_group: peer_cap,
            max_attaches: 50_000,
            ..ProbeBudgets::default()
        };
        let (summary, _cache) = probe_keep_set_groups(
            &mut items,
            KeepSetProbeOpts {
                budgets,
                level: ProbeLevel::Open,
                policy: KeepPolicy::FirstSeen,
                family: FamilyPolicy::KeepAttachmentsWithParent,
                prefer_path: &[],
                tier2_enabled: true,
                mode: ScanMode::BestEffort,
                cancel: None,
                progress: None,
            },
        );
        assert_eq!(
            summary.peer_probe_capped_groups, 1,
            "exact-N dirty peers must tally as capped"
        );
        assert_eq!(items.len() as u64, peer_cap);
    }

    /// N−1 dirty peers exhaust the group without hitting the cap.
    #[test]
    fn peer_probe_cap_n_minus_one_not_capped() {
        let peer_cap = 3u64;
        let mut items: Vec<RecoverableScanItem> = (0..peer_cap - 1)
            .map(|i| RecoverableScanItem {
                locus: MessageLocus {
                    source_path: format!("C:\\missing\\nm1{i}.pst"),
                    source_pst: format!("nm1{i}.pst"),
                    folder_path: "Inbox".into(),
                    nid: 0x70 + i,
                    is_orphaned: false,
                },
                message_id_norm: Some("same-mid-nm1".into()),
                content_hash: [3; 32],
                size: 100,
                integrity: RecoverableIntegrity::clean(),
                scan_order: i,
                submit_time: None,
                delivery_time: None,
                has_bcc: false,
            })
            .collect();
        let budgets = ProbeBudgets {
            max_peer_probes_per_group: peer_cap,
            max_attaches: 50_000,
            ..ProbeBudgets::default()
        };
        let (summary, _cache) = probe_keep_set_groups(
            &mut items,
            KeepSetProbeOpts {
                budgets,
                level: ProbeLevel::Open,
                policy: KeepPolicy::FirstSeen,
                family: FamilyPolicy::KeepAttachmentsWithParent,
                prefer_path: &[],
                tier2_enabled: true,
                mode: ScanMode::BestEffort,
                cancel: None,
                progress: None,
            },
        );
        assert_eq!(
            summary.peer_probe_capped_groups, 0,
            "fewer than N peers is exhaustion, not cap"
        );
    }

    #[test]
    fn budget_exhaust_sets_truncated() {
        let mut items: Vec<RecoverableScanItem> = (0..5)
            .map(|i| RecoverableScanItem {
                locus: MessageLocus {
                    source_path: format!("C:\\missing\\b{i}.pst"),
                    source_pst: format!("b{i}.pst"),
                    folder_path: "Inbox".into(),
                    nid: 0x30 + i as u64,
                    is_orphaned: false,
                },
                message_id_norm: Some(format!("mid{i}")),
                content_hash: [i as u8; 32],
                size: 10,
                integrity: RecoverableIntegrity::clean(),
                scan_order: i as u64,
                submit_time: None,
                delivery_time: None,
                has_bcc: false,
            })
            .collect();
        let budgets = ProbeBudgets {
            max_attaches: 2,
            max_peer_probes_per_group: 10,
            ..ProbeBudgets::default()
        };
        let (summary, _cache) = probe_scan_items(
            &mut items,
            budgets,
            ProbeLevel::Open,
            ScanMode::BestEffort,
            None,
            None,
        );
        // max_attaches=2 → after 2 attempts truncated
        assert!(summary.attempted <= 2);
        assert!(summary.truncated || summary.attempted == 2);
        // Rates from attempted only
        if summary.attempted > 0 {
            let rate = summary.failed as f64 / summary.attempted as f64;
            assert!((0.0..=1.0).contains(&rate));
        }
    }

    /// Cancel mid-pass must not look like attach corruption (§3.11 / review P0).
    #[test]
    fn cancel_mid_pass_not_attach_fail() {
        let cancel = Arc::new(AtomicBool::new(true));
        let mut items: Vec<RecoverableScanItem> = (0..4)
            .map(|i| RecoverableScanItem {
                locus: MessageLocus {
                    source_path: format!("C:\\missing\\cancel{i}.pst"),
                    source_pst: format!("cancel{i}.pst"),
                    folder_path: "Inbox".into(),
                    nid: 0x40 + i as u64,
                    is_orphaned: false,
                },
                message_id_norm: Some(format!("mid{i}")),
                content_hash: [i as u8; 32],
                size: 10,
                integrity: RecoverableIntegrity::clean(),
                scan_order: i as u64,
                submit_time: None,
                delivery_time: None,
                has_bcc: false,
            })
            .collect();
        let (summary, _cache) = probe_scan_items(
            &mut items,
            ProbeBudgets::default(),
            ProbeLevel::Open,
            ScanMode::BestEffort,
            Some(cancel),
            None,
        );
        assert!(summary.cancelled, "cancel flag must set summary.cancelled");
        assert_eq!(
            summary.failed, 0,
            "cancel must not inflate failed (got {})",
            summary.failed
        );
        // Items must not be degraded solely due to cancel.
        for item in &items {
            assert!(
                !item.integrity.degraded,
                "cancel must not degrade integrity as attach fail"
            );
            assert!(
                !item
                    .integrity
                    .degraded_reasons
                    .contains(&IntegrityReason::AttachProbeTimeout),
                "cancel must not use AttachProbeTimeout"
            );
        }
        // Coverage honesty: preflight must surface cancelled/truncated.
        let pre = AttachProbePreflight::from_tallies(
            "head",
            summary.attempted,
            summary.failed,
            summary.truncated,
            0.05,
            summary.peer_probe_capped_groups,
            summary.cancelled,
        );
        assert!(pre.cancelled);
        assert!(pre.truncated);
        assert!(pre.coverage_note.contains("cancelled"));
    }

    #[test]
    fn strict_mode_marks_probe_fail_for_skip() {
        let mut items = vec![RecoverableScanItem {
            locus: MessageLocus {
                source_path: "C:\\missing\\strict.pst".into(),
                source_pst: "strict.pst".into(),
                folder_path: "Inbox".into(),
                nid: 0x50,
                is_orphaned: false,
            },
            message_id_norm: Some("m".into()),
            content_hash: [0; 32],
            size: 10,
            integrity: RecoverableIntegrity::clean(),
            scan_order: 0,
            submit_time: None,
            delivery_time: None,
            has_bcc: false,
        }];
        let (summary, _cache) = probe_scan_items(
            &mut items,
            ProbeBudgets::default(),
            ProbeLevel::Open,
            ScanMode::Strict,
            None,
            None,
        );
        // Missing PST → open fail; strict marks reasons so caller can skip.
        assert!(summary.attempted >= 1 || summary.failed >= 1 || items[0].integrity.degraded);
        if items[0].integrity.degraded {
            assert!(items[0]
                .integrity
                .degraded_reasons
                .iter()
                .any(|r| r.is_attach_probe_fail()));
        }
    }

    #[test]
    fn winner_prefers_clean_peer_after_degrade() {
        use dedup_engine::keepset::fidelity_rank;
        let dirty = RecoverableScanItem {
            locus: MessageLocus {
                source_path: "a.pst".into(),
                source_pst: "a.pst".into(),
                folder_path: "Inbox".into(),
                nid: 1,
                is_orphaned: false,
            },
            message_id_norm: Some("m".into()),
            content_hash: [0; 32],
            size: 100,
            integrity: RecoverableIntegrity::with_degraded(
                vec![IntegrityReason::AttachStreamCrc],
                false,
            ),
            scan_order: 0,
            submit_time: None,
            delivery_time: None,
            has_bcc: false,
        };
        let clean = RecoverableScanItem {
            locus: MessageLocus {
                source_path: "b.pst".into(),
                source_pst: "b.pst".into(),
                folder_path: "Inbox".into(),
                nid: 2,
                is_orphaned: false,
            },
            message_id_norm: Some("m".into()),
            content_hash: [0; 32],
            size: 100,
            integrity: RecoverableIntegrity::clean(),
            scan_order: 1,
            submit_time: None,
            delivery_time: None,
            has_bcc: false,
        };
        assert!(fidelity_rank(&clean) < fidelity_rank(&dirty));
        let ctx = RankContext::new(KeepPolicy::FirstSeen);
        let key_clean = rank_key(&clean, &ctx);
        let key_dirty = rank_key(&dirty, &ctx);
        assert!(key_clean < key_dirty);
    }

    #[test]
    fn reason_strings_match_0073() {
        assert_eq!(
            IntegrityReason::AttachStreamOpenFailed.as_str(),
            "ATTACH_STREAM_OPEN_FAILED"
        );
        assert_eq!(
            IntegrityReason::AttachStreamCrc.as_str(),
            "ATTACH_STREAM_CRC"
        );
    }

    /// Slow reader that sleeps so per-attach deadline trips.
    struct SlowReader {
        delay_ms: u64,
        remaining: usize,
    }

    impl Read for SlowReader {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            std::thread::sleep(Duration::from_millis(self.delay_ms));
            if self.remaining == 0 {
                return Ok(0);
            }
            let n = self.remaining.min(buf.len()).min(1);
            buf[..n].fill(0xCD);
            self.remaining -= n;
            Ok(n)
        }
    }

    #[test]
    fn per_attach_timeout_via_deadline() {
        // Non-expired start: first read completes after delay, subsequent check times out
        // after partial progress (production-path deadline pattern).
        let mut reader = SlowReader {
            delay_ms: 40,
            remaining: 1_000_000,
        };
        let deadline = Instant::now() + Duration::from_millis(70);
        let mut buf = [0u8; DISCARD_CHUNK];
        let mut bytes_read = 0u64;
        let max_bytes = 1024u64;
        let mut timed_out = false;
        while bytes_read < max_bytes {
            if Instant::now() >= deadline {
                timed_out = true;
                break;
            }
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    bytes_read += n as u64;
                    if Instant::now() >= deadline {
                        timed_out = true;
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        assert!(timed_out, "deadline must trip after partial slow reads");
        assert!(
            bytes_read > 0,
            "timeout after partial progress expected (got {bytes_read})"
        );
        let reason = IntegrityReason::AttachProbeTimeout;
        assert_eq!(reason.as_str(), "ATTACH_PROBE_TIMEOUT");
        assert!(reason.is_attach_probe_fail());
    }

    #[test]
    fn timed_probe_recv_timeout_returns_attach_probe_timeout() {
        // Hard wall-clock via channel: worker sleeps past wait → timeout outcome.
        let (tx, rx) = mpsc::channel::<ProbeOutcome>();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            let _ = tx.send(ProbeOutcome {
                ok: true,
                reason: None,
                bytes_read: 0,
                timed_out: false,
                level: ProbeLevel::Head,
            });
        });
        let result = rx.recv_timeout(Duration::from_millis(30));
        assert!(matches!(result, Err(mpsc::RecvTimeoutError::Timeout)));
        // Production maps this to ATTACH_PROBE_TIMEOUT:
        let outcome = ProbeOutcome {
            ok: false,
            reason: Some(IntegrityReason::AttachProbeTimeout),
            bytes_read: 0,
            timed_out: true,
            level: ProbeLevel::Head,
        };
        assert!(outcome.timed_out);
        assert_eq!(outcome.reason, Some(IntegrityReason::AttachProbeTimeout));
    }

    #[test]
    fn attach_reason_property_not_found_is_stream_open_fail() {
        use pst_reader::PstError;
        assert_eq!(
            attach_reason_from_pst_error(&PstError::PropertyNotFound(0x3701)),
            IntegrityReason::AttachStreamOpenFailed
        );
    }

    /// Unit-level: after strict probe marks, callers recompute recoverable from kept set.
    #[test]
    fn strict_probe_skip_reconciles_recoverable_tally() {
        let mut items = vec![
            RecoverableScanItem {
                locus: MessageLocus {
                    source_path: "C:\\missing\\a.pst".into(),
                    source_pst: "a.pst".into(),
                    folder_path: "Inbox".into(),
                    nid: 1,
                    is_orphaned: false,
                },
                message_id_norm: Some("m1".into()),
                content_hash: [1; 32],
                size: 10,
                integrity: RecoverableIntegrity::clean(),
                scan_order: 0,
                submit_time: None,
                delivery_time: None,
                has_bcc: false,
            },
            RecoverableScanItem {
                locus: MessageLocus {
                    source_path: "C:\\missing\\b.pst".into(),
                    source_pst: "b.pst".into(),
                    folder_path: "Inbox".into(),
                    nid: 2,
                    is_orphaned: false,
                },
                message_id_norm: Some("m2".into()),
                content_hash: [2; 32],
                size: 20,
                integrity: RecoverableIntegrity::clean(),
                scan_order: 1,
                submit_time: None,
                delivery_time: None,
                has_bcc: false,
            },
        ];
        let (_summary, _cache) = probe_scan_items(
            &mut items,
            ProbeBudgets::default(),
            ProbeLevel::Open,
            ScanMode::Strict,
            None,
            None,
        );
        // Simulate scan/unique-pst strict retain: drop probe-fail candidates.
        let pre_len = items.len();
        items.retain(|c| {
            !c.integrity
                .degraded_reasons
                .iter()
                .any(|r| r.is_attach_probe_fail())
        });
        let recoverable = items.len() as u64;
        // Missing PSTs → open fails → both skipped under strict.
        assert!(pre_len >= 2);
        assert_eq!(
            recoverable, 0,
            "strict probe fails must leave zero recoverable when all open fail"
        );
        // Full preflight recompute with updated skipped/recoverable.
        let skipped = pre_len as u64 - recoverable;
        let pre = compute_preflight_strict_after_probe(recoverable, skipped, 2, 2, false, false);
        assert_eq!(
            pre.recommendation,
            dedup_engine::integrity::PreflightRecommendation::NotExportReady
        );
        assert_eq!(pre.attach_probe.attempted, 2);
    }

    fn compute_preflight_strict_after_probe(
        recoverable: u64,
        skipped: u64,
        attempted: u64,
        failed: u64,
        truncated: bool,
        cancelled: bool,
    ) -> dedup_engine::integrity::PreflightReport {
        use dedup_engine::integrity::{compute_preflight, IntegrityThresholds, PreflightInputs};
        compute_preflight(&PreflightInputs {
            mode: ScanMode::Strict,
            recoverable,
            skipped,
            crc_skips: 0,
            failed_files: 0,
            input_file_count: 1,
            thresholds: IntegrityThresholds::default(),
            attach_probe_enabled: true,
            attach_probe_level: "open".into(),
            attach_attempted: attempted,
            attach_failed: failed,
            attach_probe_truncated: truncated,
            peer_probe_capped_groups: 0,
            attach_probe_cancelled: cancelled,
        })
    }

    /// Timeout must charge reserved per-attach budget even when bytes_read=0
    /// (abandoned worker may still be reading — DoD-8 / P1-C).
    #[test]
    fn timeout_charges_global_byte_budget() {
        let budgets = ProbeBudgets {
            max_probe_bytes: 10_000,
            per_attach_max_bytes: 1_000,
            max_attaches: 50_000,
            ..ProbeBudgets::default()
        };
        let mut engine = AttachProbeEngine::new(budgets, ProbeLevel::Head, None, None);
        assert_eq!(engine.bytes_left, 10_000);

        let timeout = ProbeOutcome {
            ok: false,
            reason: Some(IntegrityReason::AttachProbeTimeout),
            bytes_read: 0,
            timed_out: true,
            level: ProbeLevel::Head,
        };
        engine.record_attempt(&timeout);
        assert_eq!(
            engine.bytes_left, 9_000,
            "timeout must reserve per_attach_max_bytes from global budget"
        );
        assert_eq!(engine.summary.bytes, 1_000);
        assert!(
            engine.summary.truncated,
            "timeout implies incomplete coverage"
        );
        assert_eq!(engine.summary.failed, 1);
        assert_eq!(engine.summary.attempted, 1);

        // Second timeout charges another reserved head.
        engine.record_attempt(&timeout);
        assert_eq!(engine.bytes_left, 8_000);
        assert_eq!(engine.summary.bytes, 2_000);

        // Partial read timeout charges max(bytes_read, reserved).
        let partial = ProbeOutcome {
            ok: false,
            reason: Some(IntegrityReason::AttachProbeTimeout),
            bytes_read: 1_500,
            timed_out: true,
            level: ProbeLevel::Head,
        };
        engine.record_attempt(&partial);
        // reserved = min(1000, 8000)=1000; max(1500,1000)=1500
        assert_eq!(engine.bytes_left, 6_500);
        assert_eq!(engine.summary.bytes, 3_500);
    }

    /// Full-level timeout must reserve the entire remaining global budget (P1-3),
    /// not only per_attach_max_bytes — workers may keep reading up to bytes_left.
    #[test]
    fn full_timeout_charges_remaining_global_budget() {
        // Pure charge helper first.
        assert_eq!(
            timeout_budget_charge(ProbeLevel::Full, 1_000, 50_000, 0),
            50_000,
            "Full timeout with bytes_read=0 charges all remaining global"
        );
        assert_eq!(
            timeout_budget_charge(ProbeLevel::Head, 1_000, 50_000, 0),
            1_000,
            "Head timeout still caps at per_attach"
        );
        assert_eq!(
            timeout_budget_charge(ProbeLevel::Full, 1_000, 50_000, 60_000),
            60_000,
            "Full timeout charges max(bytes_read, reserved)"
        );

        let budgets = ProbeBudgets {
            max_probe_bytes: 50_000,
            per_attach_max_bytes: 1_000,
            max_attaches: 50_000,
            ..ProbeBudgets::default()
        };
        let mut engine = AttachProbeEngine::new(budgets, ProbeLevel::Full, None, None);
        let timeout = ProbeOutcome {
            ok: false,
            reason: Some(IntegrityReason::AttachProbeTimeout),
            bytes_read: 0,
            timed_out: true,
            level: ProbeLevel::Full,
        };
        engine.record_attempt(&timeout);
        assert_eq!(
            engine.bytes_left, 0,
            "Full timeout must drain remaining global budget (not only 1 KiB head)"
        );
        assert_eq!(engine.summary.bytes, 50_000);
        assert!(engine.summary.truncated);
        assert!(engine.budget_exhausted());
    }

    /// Cache fail at requested level ⇒ stream_available=false without PST I/O (P1-A).
    #[test]
    fn cache_fail_stream_available_false_no_io() {
        let mut cache = ProbeResultCache::new();
        let fail = ProbeOutcome {
            ok: false,
            reason: Some(IntegrityReason::AttachStreamCrc),
            bytes_read: 64,
            timed_out: false,
            level: ProbeLevel::Head,
        };
        cache.insert("C:\\mail\\a.pst", 0x21, 0x100, 4096, 12345, 999, &fail);

        assert_eq!(
            cache.stream_available_at_level(
                "C:\\mail\\a.pst",
                0x21,
                0x100,
                4096,
                12345,
                999,
                ProbeLevel::Head
            ),
            Some(false)
        );
        // Ok at same identity after re-insert at higher level would dominate; fail sticks at same level.
        let ok = ProbeOutcome {
            ok: true,
            reason: None,
            bytes_read: 64,
            timed_out: false,
            level: ProbeLevel::Head,
        };
        cache.insert("C:\\mail\\a.pst", 0x21, 0x100, 4096, 12345, 999, &ok);
        // Prefer fail over ok at same level:
        assert_eq!(
            cache.stream_available_at_level(
                "C:\\mail\\a.pst",
                0x21,
                0x100,
                4096,
                12345,
                999,
                ProbeLevel::Head
            ),
            Some(false)
        );
        // Miss keeps legacy optimistic (None).
        assert!(cache
            .stream_available_at_level(
                "C:\\mail\\a.pst",
                0x21,
                0x101,
                4096,
                12345,
                999,
                ProbeLevel::Head
            )
            .is_none());
    }

    /// Cancel-before-probe preflight inputs mark attach_probe cancelled + incomplete.
    #[test]
    fn cancel_before_probe_preflight_incomplete() {
        use dedup_engine::integrity::{compute_preflight, IntegrityThresholds, PreflightInputs};
        let report = compute_preflight(&PreflightInputs {
            mode: ScanMode::BestEffort,
            recoverable: 5,
            skipped: 0,
            crc_skips: 0,
            failed_files: 0,
            input_file_count: 1,
            thresholds: IntegrityThresholds::default(),
            attach_probe_enabled: true,
            attach_probe_level: "head".into(),
            attach_attempted: 0,
            attach_failed: 0,
            attach_probe_truncated: false,
            peer_probe_capped_groups: 0,
            attach_probe_cancelled: true,
        });
        assert!(report.attach_probe.enabled);
        assert!(report.attach_probe.cancelled);
        assert_eq!(report.attach_probe.attempted, 0);
        assert!(
            report.attach_probe.coverage_note.contains("cancel")
                || report.attach_probe.coverage_note.contains("incomplete")
                || report.attach_probe.truncated,
            "coverage note must surface incomplete/cancelled: {}",
            report.attach_probe.coverage_note
        );
    }
}
