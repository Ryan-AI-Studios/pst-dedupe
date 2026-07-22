//! Expand resource limits and checkpoint cadence.

use serde::{Deserialize, Serialize};

/// Configurable guards for ZIP expand + resume durability.
///
/// Defaults are production-oriented (large Purview packages). Tests should
/// override with tiny values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExpandLimits {
    /// Max total uncompressed bytes CAS'd per ingest job.
    pub max_uncompressed_bytes: u64,
    /// Max uncompressed/compressed ratio per entry (when compressed size > 0).
    pub max_compression_ratio: f64,
    /// Max leaf entries processed per job (files, not directories).
    pub max_entries: u64,
    /// Max nested ZIP depth (outer package ZIP = depth 1).
    pub max_zip_depth: u32,
    /// Write expand checkpoint after this many successful leaf commits.
    pub checkpoint_every_n_entries: u64,
    /// Write expand checkpoint after this many successful uncompressed bytes.
    pub checkpoint_every_bytes: u64,
    /// Max size of a **single leaf** (e.g. multi-GB PST). Enforced without holding
    /// the whole object in RAM when streamed via [`crate::expand::ExpandSession::commit_leaf_reader`].
    ///
    /// eDiscovery mailboxes routinely reach several GiB; default allows large PSTs
    /// while still capping pathological multi-tens-of-GiB members.
    pub max_entry_bytes: u64,
    /// Cap for loading a single entry **fully into a `Vec`** (nested ZIP materialize
    /// and other full-buffer paths only). Streaming leaf CAS does **not** use this.
    pub max_entry_buffer_bytes: u64,
}

impl Default for ExpandLimits {
    fn default() -> Self {
        Self {
            max_uncompressed_bytes: 50 * 1024 * 1024 * 1024, // 50 GiB
            max_compression_ratio: 100.0,
            max_entries: 500_000,
            max_zip_depth: 8,
            checkpoint_every_n_entries: 50,
            checkpoint_every_bytes: 64 * 1024 * 1024, // 64 MiB
            // Multi-GB PSTs are normal; stream to CAS (see commit_leaf_reader).
            max_entry_bytes: 20 * 1024 * 1024 * 1024, // 20 GiB
            // Nested ZIP re-walk still materializes the container zip in memory/temp.
            max_entry_buffer_bytes: 512 * 1024 * 1024, // 512 MiB
        }
    }
}

impl ExpandLimits {
    /// Tight limits for unit/integration tests.
    pub fn for_tests() -> Self {
        Self {
            max_uncompressed_bytes: 16 * 1024 * 1024,
            max_compression_ratio: 100.0,
            max_entries: 10_000,
            max_zip_depth: 8,
            checkpoint_every_n_entries: 1,
            checkpoint_every_bytes: 1024,
            max_entry_bytes: 8 * 1024 * 1024,
            max_entry_buffer_bytes: 8 * 1024 * 1024,
        }
    }
}
