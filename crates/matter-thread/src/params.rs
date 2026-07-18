//! Job params for matter-level email threading.

use serde::{Deserialize, Serialize};

/// JSON params for kind `"thread"`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadParams {
    /// Build Message-ID / In-Reply-To / References graph (default true).
    #[serde(default = "default_true")]
    pub use_headers: bool,
    /// Subject fallback among remaining singletons (default true).
    #[serde(default = "default_true")]
    pub use_subject_fallback: bool,
    /// ConversationIndex opaque prefix among remaining singletons (default true).
    #[serde(default = "default_true")]
    pub use_conversation_index: bool,
    /// Clear prior `thread_*` result fields then full recompute (default true).
    #[serde(default = "default_true")]
    pub reset: bool,
    /// Commit batch size for thread updates + checkpoint (default 500).
    #[serde(default = "default_batch_size")]
    pub batch_size: u64,
    /// Copy parent thread fields onto attachment children (default true).
    #[serde(default = "default_true")]
    pub family_inherit: bool,
}

fn default_true() -> bool {
    true
}

fn default_batch_size() -> u64 {
    500
}

impl Default for ThreadParams {
    fn default() -> Self {
        Self {
            use_headers: true,
            use_subject_fallback: true,
            use_conversation_index: true,
            reset: true,
            batch_size: 500,
            family_inherit: true,
        }
    }
}

impl ThreadParams {
    /// Parse from JSON, applying defaults for missing keys.
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        if json.trim().is_empty() {
            return Ok(Self::default());
        }
        serde_json::from_str(json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_from_empty_object() {
        let p = ThreadParams::from_json("{}").unwrap();
        assert!(p.use_headers);
        assert!(p.use_subject_fallback);
        assert!(p.use_conversation_index);
        assert!(p.reset);
        assert_eq!(p.batch_size, 500);
        assert!(p.family_inherit);
    }

    #[test]
    fn parse_overrides() {
        let p = ThreadParams::from_json(
            r#"{"use_headers":false,"use_subject_fallback":false,"batch_size":10,"reset":false}"#,
        )
        .unwrap();
        assert!(!p.use_headers);
        assert!(!p.use_subject_fallback);
        assert_eq!(p.batch_size, 10);
        assert!(!p.reset);
    }
}
