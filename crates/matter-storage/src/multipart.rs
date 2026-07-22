//! Multipart upload caps — O(1) RAM for multi-GB streams (LOCKED §3.3.2).

use crate::error::{Result, StorageError};

/// Default part size: 10 MiB.
pub const DEFAULT_PART_SIZE: usize = 10 * 1024 * 1024;

/// Hard max part size: 16 MiB (config may not exceed).
pub const MAX_PART_SIZE: usize = 16 * 1024 * 1024;

/// Default concurrent part uploads.
pub const DEFAULT_CONCURRENT_PARTS: usize = 2;

/// Hard max concurrent part uploads.
pub const MAX_CONCURRENT_PARTS: usize = 2;

/// Peak RAM target: `part_size × concurrent` ≈ 20 MiB with defaults.
pub const DEFAULT_PEAK_RAM_TARGET: usize = DEFAULT_PART_SIZE * DEFAULT_CONCURRENT_PARTS;

/// Validated multipart limits for cloud puts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MultipartLimits {
    pub part_size: usize,
    pub max_concurrent: usize,
}

impl Default for MultipartLimits {
    fn default() -> Self {
        Self {
            part_size: DEFAULT_PART_SIZE,
            max_concurrent: DEFAULT_CONCURRENT_PARTS,
        }
    }
}

impl MultipartLimits {
    /// Validate and construct. Fail-closed if over hard caps.
    pub fn new(part_size: usize, max_concurrent: usize) -> Result<Self> {
        if part_size == 0 {
            return Err(StorageError::Config(
                "multipart part_size must be > 0".into(),
            ));
        }
        if part_size > MAX_PART_SIZE {
            return Err(StorageError::Config(format!(
                "multipart part_size {part_size} exceeds hard max {MAX_PART_SIZE} (OOM protection)"
            )));
        }
        if max_concurrent == 0 {
            return Err(StorageError::Config(
                "multipart max_concurrent must be > 0".into(),
            ));
        }
        if max_concurrent > MAX_CONCURRENT_PARTS {
            return Err(StorageError::Config(format!(
                "multipart max_concurrent {max_concurrent} exceeds hard max {MAX_CONCURRENT_PARTS}"
            )));
        }
        Ok(Self {
            part_size,
            max_concurrent,
        })
    }

    /// Approximate peak upload buffer budget.
    pub fn peak_buffer_bytes(&self) -> usize {
        self.part_size.saturating_mul(self.max_concurrent)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_within_caps() {
        let l = MultipartLimits::default();
        assert!(l.part_size <= MAX_PART_SIZE);
        assert!(l.max_concurrent <= MAX_CONCURRENT_PARTS);
        assert!(l.peak_buffer_bytes() <= DEFAULT_PEAK_RAM_TARGET);
    }

    #[test]
    fn rejects_huge_part() {
        assert!(MultipartLimits::new(100 * 1024 * 1024, 1).is_err());
    }

    #[test]
    fn rejects_unbounded_concurrency() {
        assert!(MultipartLimits::new(1024 * 1024, 64).is_err());
    }
}
