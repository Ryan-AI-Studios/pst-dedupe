//! Cooperative cancel token (`Arc<AtomicBool>`).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Cloneable cancel flag shared between UI/CLI and the matter worker.
///
/// Cancellation is **cooperative only** — handlers must poll
/// [`CancelToken::is_cancelled`] (or [`CancelToken::as_fn`]) between units of
/// work. Stages set the job to **Paused** on cancel.
#[derive(Debug, Clone, Default)]
pub struct CancelToken {
    flag: Arc<AtomicBool>,
}

impl CancelToken {
    /// Create a fresh, not-cancelled token.
    pub fn new() -> Self {
        Self {
            flag: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Request cooperative cancellation.
    pub fn cancel(&self) {
        self.flag.store(true, Ordering::SeqCst);
    }

    /// Whether cancellation has been requested.
    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }

    /// Closure form for stage APIs that take `Option<&dyn Fn() -> bool>`.
    pub fn as_fn(&self) -> impl Fn() -> bool + '_ {
        move || self.is_cancelled()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancel_is_visible() {
        let t = CancelToken::new();
        assert!(!t.is_cancelled());
        t.cancel();
        assert!(t.is_cancelled());
        assert!(t.as_fn()());
    }
}
