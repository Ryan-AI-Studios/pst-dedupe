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
    /// Soft cap for loading a single entry fully into memory before CAS put.
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
            max_entry_buffer_bytes: 256 * 1024 * 1024, // 256 MiB
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
            max_entry_buffer_bytes: 8 * 1024 * 1024,
        }
    }
}
