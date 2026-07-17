//! Runner configuration.

/// Configuration for [`crate::ProcessRunner`].
#[derive(Debug, Clone)]
pub struct RunnerConfig {
    /// Enable optional `broadcast` event stream (full event log for CLI/tests).
    pub enable_broadcast: bool,
    /// Capacity of the broadcast channel when enabled.
    pub broadcast_capacity: usize,
}

impl Default for RunnerConfig {
    fn default() -> Self {
        Self {
            enable_broadcast: true,
            broadcast_capacity: 256,
        }
    }
}

impl RunnerConfig {
    /// Config with broadcast disabled (watch-only).
    pub fn watch_only() -> Self {
        Self {
            enable_broadcast: false,
            broadcast_capacity: 0,
        }
    }
}
