//! Progress snapshots via `tokio::sync::watch` (latest-only) and optional broadcast.

use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, watch};

/// Latest progress view for UI progress bars.
///
/// `watch` holds only the **latest** snapshot. Multiple subscribers all see
/// the freshest state; there is no MPMC message steal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobProgressSnapshot {
    pub job_id: String,
    pub kind: String,
    pub matter_id: String,
    /// Wire form of [`matter_core::JobState`] (`pending`, `running`, …).
    pub state: String,
    pub stage: Option<String>,
    pub completed_count: u64,
    pub total_hint: Option<u64>,
    pub message: Option<String>,
    pub error_summary: Option<String>,
    pub updated_at: String,
}

impl JobProgressSnapshot {
    /// Empty / idle snapshot before any job runs.
    pub fn idle() -> Self {
        Self {
            job_id: String::new(),
            kind: String::new(),
            matter_id: String::new(),
            state: "idle".into(),
            stage: None,
            completed_count: 0,
            total_hint: None,
            message: None,
            error_summary: None,
            updated_at: now_rfc3339_approx(),
        }
    }

    /// Whether this snapshot is a terminal job state.
    pub fn is_terminal(&self) -> bool {
        matches!(
            self.state.as_str(),
            "succeeded" | "failed" | "cancelled" | "paused"
        )
    }
}

/// Discrete progress event for optional full event-stream consumers (CLI/tests).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProgressEvent {
    pub snapshot: JobProgressSnapshot,
}

/// Sink handlers use to publish progress (updates watch + optional broadcast).
#[derive(Clone)]
pub struct ProgressSink {
    watch_tx: watch::Sender<JobProgressSnapshot>,
    event_tx: Option<broadcast::Sender<ProgressEvent>>,
}

impl ProgressSink {
    pub(crate) fn new(
        watch_tx: watch::Sender<JobProgressSnapshot>,
        event_tx: Option<broadcast::Sender<ProgressEvent>>,
    ) -> Self {
        Self { watch_tx, event_tx }
    }

    /// Publish a new latest snapshot (non-blocking overwrite for watch).
    pub fn update(&self, snapshot: JobProgressSnapshot) {
        let _ = self.watch_tx.send(snapshot.clone());
        if let Some(ref tx) = self.event_tx {
            let _ = tx.send(ProgressEvent { snapshot });
        }
    }

    /// Convenience: patch fields on the current snapshot and publish.
    pub fn patch<F>(&self, f: F)
    where
        F: FnOnce(&mut JobProgressSnapshot),
    {
        let mut snap = self.watch_tx.borrow().clone();
        f(&mut snap);
        snap.updated_at = now_rfc3339_approx();
        self.update(snap);
    }
}

pub(crate) fn now_rfc3339_approx() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Compact RFC3339-ish UTC without chrono dependency in this crate.
    format!("{secs}")
}

/// Create a watch channel seeded with an idle snapshot.
pub fn progress_channel() -> (
    watch::Sender<JobProgressSnapshot>,
    watch::Receiver<JobProgressSnapshot>,
) {
    watch::channel(JobProgressSnapshot::idle())
}
