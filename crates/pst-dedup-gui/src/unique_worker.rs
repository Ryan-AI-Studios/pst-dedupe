//! Background worker for unique-PST export (0072).
//!
//! Runs [`pst_dedup_cli::run_unique_pst_with_options`] on a dedicated thread.
//! Progress / log callbacks update shared state and call `ctx.request_repaint()`
//! so the UI updates without mouse motion.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui;
use pst_dedup_cli::unique_pst_cmd::{
    run_unique_pst_with_options, UniquePstCliArgs, UniquePstOutcome, UniquePstProgress,
    UniquePstRunOptions, UniqueVolumeDigest,
};

/// Cap log lines retained for the Details panel.
const MAX_LOG_LINES: usize = 2000;

/// Shared progress / log state for the unique-PST wizard Run screen.
#[derive(Debug, Clone, Default)]
pub struct UniqueProgressState {
    pub stage: String,
    pub volume_index: u32,
    /// Messages written on the current volume.
    pub messages_written: u64,
    /// Cumulative across volumes (preferred for progress bar).
    pub messages_written_cumulative: u64,
    pub physical_bytes: u64,
    pub winners_total: Option<u64>,
    pub log_lines: Vec<String>,
    pub cancelled: bool,
    pub complete: bool,
    pub error: Option<String>,
    pub outcome: Option<UniqueOutcomeView>,
}

/// Per-volume row for the Done screen.
#[derive(Debug, Clone)]
pub struct VolumeDigestView {
    pub volume_index: u32,
    pub path: String,
    pub bytes: u64,
    pub messages_written: u64,
    pub sha256_hex: String,
    pub md5_hex: String,
}

impl From<UniqueVolumeDigest> for VolumeDigestView {
    fn from(v: UniqueVolumeDigest) -> Self {
        Self {
            volume_index: v.volume_index,
            path: v.path,
            bytes: v.bytes,
            messages_written: v.messages_written,
            sha256_hex: v.sha256_hex,
            md5_hex: v.md5_hex,
        }
    }
}

/// GUI-friendly subset of [`UniquePstOutcome`].
#[derive(Debug, Clone)]
pub struct UniqueOutcomeView {
    pub ok: bool,
    pub cancelled: bool,
    pub report_dir: std::path::PathBuf,
    pub summary_path: std::path::PathBuf,
    pub out: std::path::PathBuf,
    pub messages_written_total: u64,
    pub unique: u64,
    pub volume_count: usize,
    pub volumes: Vec<VolumeDigestView>,
    pub error_message: Option<String>,
}

impl From<UniquePstOutcome> for UniqueOutcomeView {
    fn from(o: UniquePstOutcome) -> Self {
        Self {
            ok: o.ok,
            cancelled: o.cancelled,
            report_dir: o.report_dir,
            summary_path: o.summary_path,
            out: o.out,
            messages_written_total: o.messages_written_total,
            unique: o.unique,
            volume_count: o.volume_count,
            volumes: o.volumes.into_iter().map(VolumeDigestView::from).collect(),
            error_message: o.error_message,
        }
    }
}

/// Throttle helper so we can still wake UI frequently without flooding egui.
///
/// Production progress path in [`run_unique_pst_worker`] uses this type.
#[derive(Debug)]
pub struct RepaintThrottle {
    last: Instant,
    min_interval: Duration,
    /// Counts every should_repaint evaluation (for unit tests).
    pub eval_count: u64,
    /// Counts every time a repaint is actually allowed.
    pub repaint_count: u64,
}

impl RepaintThrottle {
    pub fn new() -> Self {
        Self {
            last: Instant::now()
                .checked_sub(Duration::from_secs(1))
                .unwrap_or_else(Instant::now),
            min_interval: Duration::from_millis(50), // ~20 Hz
            eval_count: 0,
            repaint_count: 0,
        }
    }

    /// Returns true when a repaint should fire (first call, forced, or interval elapsed).
    pub fn should_repaint(&mut self, force: bool) -> bool {
        self.eval_count = self.eval_count.saturating_add(1);
        if force || self.last.elapsed() >= self.min_interval {
            self.repaint_count = self.repaint_count.saturating_add(1);
            self.last = Instant::now();
            true
        } else {
            false
        }
    }
}

impl Default for RepaintThrottle {
    fn default() -> Self {
        Self::new()
    }
}

/// Apply a progress tick to shared state (production path used by the worker).
pub fn apply_unique_progress(progress: &Arc<Mutex<UniqueProgressState>>, tick: UniquePstProgress) {
    let mut p = progress.lock().unwrap_or_else(|e| e.into_inner());
    p.stage = tick.stage;
    p.volume_index = tick.volume_index;
    p.messages_written = tick.messages_written;
    p.messages_written_cumulative = tick.messages_written_cumulative;
    p.physical_bytes = tick.physical_bytes;
    p.winners_total = tick.winners_total;
}

/// Append a log line to shared state (production path used by the worker).
pub fn apply_unique_log(progress: &Arc<Mutex<UniqueProgressState>>, line: String) {
    let mut p = progress.lock().unwrap_or_else(|e| e.into_inner());
    p.log_lines.push(line);
    if p.log_lines.len() > MAX_LOG_LINES {
        let excess = p.log_lines.len() - MAX_LOG_LINES;
        p.log_lines.drain(0..excess);
    }
}

/// **Exact production progress path** used by [`run_unique_pst_worker`]:
/// update shared state, then throttle-gated `request_repaint` (via callback).
///
/// Tests must call this function (not a hand-rolled duplicate) so removing the
/// production wake would fail the unit suite.
pub fn production_progress_tick(
    progress: &Arc<Mutex<UniqueProgressState>>,
    throttle: &mut RepaintThrottle,
    tick: UniquePstProgress,
    mut request_repaint: impl FnMut(),
) {
    apply_unique_progress(progress, tick);
    if throttle.should_repaint(false) {
        request_repaint();
    }
}

/// **Exact production log path** used by [`run_unique_pst_worker`].
pub fn production_log_line(
    progress: &Arc<Mutex<UniqueProgressState>>,
    throttle: &mut RepaintThrottle,
    line: String,
    mut request_repaint: impl FnMut(),
) {
    apply_unique_log(progress, line);
    if throttle.should_repaint(true) {
        request_repaint();
    }
}

/// Spawn unique-pst on a worker thread. Returns join handle.
///
/// `cancel` is shared with the Cancel button. `ctx` is cloned for repaint wakes.
pub fn spawn_unique_pst(
    args: UniquePstCliArgs,
    progress: Arc<Mutex<UniqueProgressState>>,
    cancel: Arc<AtomicBool>,
    ctx: egui::Context,
) -> std::thread::JoinHandle<UniqueOutcomeView> {
    std::thread::spawn(move || run_unique_pst_worker(args, progress, cancel, ctx, None))
}

/// Core worker (also used by unit tests with a repaint hook instead of egui).
pub fn run_unique_pst_worker(
    args: UniquePstCliArgs,
    progress: Arc<Mutex<UniqueProgressState>>,
    cancel: Arc<AtomicBool>,
    ctx: egui::Context,
    on_repaint: Option<Box<dyn FnMut() + Send>>,
) -> UniqueOutcomeView {
    {
        let mut p = progress.lock().unwrap_or_else(|e| e.into_inner());
        *p = UniqueProgressState {
            stage: "starting".into(),
            ..Default::default()
        };
    }

    let progress_p = Arc::clone(&progress);
    let progress_l = Arc::clone(&progress);
    // Production throttle — same helpers unit-tested via `production_progress_tick`.
    let mut throttle = RepaintThrottle::new();
    let mut throttle_log = RepaintThrottle::new();
    let ctx_p = ctx.clone();
    let ctx_l = ctx.clone();
    // Optional test seam: count/observe repaint wakes without requiring a display.
    let repaint_hook = Arc::new(Mutex::new(on_repaint));
    let repaint_hook_p = Arc::clone(&repaint_hook);
    let repaint_hook_l = Arc::clone(&repaint_hook);

    let run_opts = UniquePstRunOptions {
        cancel: Some(Arc::clone(&cancel)),
        stderr_progress: false,
        on_progress: Some(Box::new(move |tick: UniquePstProgress| {
            production_progress_tick(&progress_p, &mut throttle, tick, || {
                ctx_p.request_repaint();
                if let Ok(mut g) = repaint_hook_p.lock() {
                    if let Some(cb) = g.as_mut() {
                        cb();
                    }
                }
            });
        })),
        on_log: Some(Box::new(move |line: String| {
            production_log_line(&progress_l, &mut throttle_log, line, || {
                ctx_l.request_repaint();
                if let Ok(mut g) = repaint_hook_l.lock() {
                    if let Some(cb) = g.as_mut() {
                        cb();
                    }
                }
            });
        })),
    };

    let result = run_unique_pst_with_options(args, run_opts);

    let view = match result {
        Ok(outcome) => UniqueOutcomeView::from(outcome),
        Err(e) => UniqueOutcomeView {
            ok: false,
            cancelled: cancel.load(Ordering::SeqCst),
            report_dir: std::path::PathBuf::new(),
            summary_path: std::path::PathBuf::new(),
            out: std::path::PathBuf::new(),
            messages_written_total: 0,
            unique: 0,
            volume_count: 0,
            volumes: vec![],
            error_message: Some(e.to_string()),
        },
    };

    {
        let mut p = progress.lock().unwrap_or_else(|e| e.into_inner());
        p.complete = true;
        p.cancelled = view.cancelled;
        p.error = view.error_message.clone();
        p.outcome = Some(view.clone());
        if view.cancelled {
            p.stage = "cancelled".into();
        } else if view.ok {
            p.stage = "done".into();
        } else {
            p.stage = "failed".into();
        }
    }
    ctx.request_repaint();
    view
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU64;

    #[test]
    fn repaint_throttle_first_call_allows_then_rapid_blocked() {
        let mut t = RepaintThrottle::new();
        // First call always requests repaint.
        assert!(t.should_repaint(false));
        assert_eq!(t.repaint_count, 1);
        assert_eq!(t.eval_count, 1);

        // Rapid subsequent calls within 50ms must not exceed throttle.
        let mut extra_allowed = 0u64;
        for _ in 0..20 {
            if t.should_repaint(false) {
                extra_allowed += 1;
            }
        }
        assert_eq!(
            extra_allowed, 0,
            "rapid calls within min_interval must be throttled"
        );
        assert_eq!(t.repaint_count, 1);
        assert_eq!(t.eval_count, 21);

        // Force bypasses throttle.
        assert!(t.should_repaint(true));
        assert_eq!(t.repaint_count, 2);
    }

    #[test]
    fn repaint_throttle_allows_after_interval() {
        let mut t = RepaintThrottle::new();
        assert!(t.should_repaint(false));
        std::thread::sleep(Duration::from_millis(60));
        assert!(t.should_repaint(false));
        assert_eq!(t.repaint_count, 2);
    }

    #[test]
    fn production_progress_tick_is_the_worker_path() {
        // Must call `production_progress_tick` (exact worker helper) — not a
        // hand-rolled apply+should_repaint duplicate that could drift.
        let progress = Arc::new(Mutex::new(UniqueProgressState::default()));
        let mut throttle = RepaintThrottle::new();
        let repaints = Arc::new(AtomicU64::new(0));
        let r = Arc::clone(&repaints);

        let tick = |stage: &str, n: u64| UniquePstProgress {
            stage: stage.into(),
            volume_index: 1,
            messages_written: n,
            messages_written_cumulative: n,
            physical_bytes: n * 100,
            winners_total: Some(10),
        };

        production_progress_tick(&progress, &mut throttle, tick("write", 1), || {
            r.fetch_add(1, Ordering::SeqCst);
        });
        {
            let p = progress.lock().unwrap_or_else(|e| e.into_inner());
            assert_eq!(p.stage, "write");
            assert_eq!(p.messages_written, 1);
        }
        assert_eq!(repaints.load(Ordering::SeqCst), 1);

        let r2 = Arc::clone(&repaints);
        for i in 2..=10 {
            production_progress_tick(&progress, &mut throttle, tick("write", i), || {
                r2.fetch_add(1, Ordering::SeqCst);
            });
        }
        {
            let p = progress.lock().unwrap_or_else(|e| e.into_inner());
            assert_eq!(p.messages_written, 10);
            assert_eq!(p.messages_written_cumulative, 10);
        }
        assert_eq!(
            repaints.load(Ordering::SeqCst),
            1,
            "production_progress_tick must throttle rapid wakes"
        );
    }

    #[test]
    fn production_log_line_forces_repaint() {
        let progress = Arc::new(Mutex::new(UniqueProgressState::default()));
        let mut throttle = RepaintThrottle::new();
        // Consume the free first interval.
        assert!(throttle.should_repaint(false));
        let repaints = Arc::new(AtomicU64::new(0));
        let r = Arc::clone(&repaints);
        production_log_line(
            &progress,
            &mut throttle,
            "unique-pst: warning: attach fail".into(),
            || {
                r.fetch_add(1, Ordering::SeqCst);
            },
        );
        assert_eq!(
            repaints.load(Ordering::SeqCst),
            1,
            "log path must force repaint even inside throttle window"
        );
        let p = progress.lock().unwrap_or_else(|e| e.into_inner());
        assert!(p.log_lines.iter().any(|l| l.contains("attach fail")));
    }

    #[test]
    fn apply_unique_log_appears_in_buffer() {
        let progress = Arc::new(Mutex::new(UniqueProgressState::default()));
        apply_unique_log(&progress, "unique-pst: warning: soft attach fail".into());
        let p = progress.lock().unwrap_or_else(|e| e.into_inner());
        assert!(p.log_lines.iter().any(|l| l.contains("soft attach")));
    }

    #[test]
    fn volume_digest_view_preserves_full_hashes() {
        let v = VolumeDigestView::from(UniqueVolumeDigest {
            volume_index: 1,
            path: "out.pst".into(),
            bytes: 100,
            messages_written: 2,
            sha256_hex: "a".repeat(64),
            md5_hex: "b".repeat(32),
        });
        assert_eq!(v.sha256_hex.len(), 64);
        assert_eq!(v.md5_hex.len(), 32);
        assert!(!v.sha256_hex.contains('…'));
    }
}
