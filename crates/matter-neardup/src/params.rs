//! Job params for matter-level near-duplicate detection.

use serde::{Deserialize, Serialize};

/// Frozen method tag for P0 MinHash + shingle near-dup.
pub const NEAR_DUP_METHOD: &str = "minhash_shingle_v1";

/// Fixed hash seed for `minhash_shingle_v1` (never random per run).
///
/// ASCII mnemonic: `ND_mh_v1` packed into little-endian-ish hex digits.
pub const DEFAULT_HASH_SEED: u64 = 0x4E44_5F6D_685F_7631;

/// JSON params for kind `"neardup"`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NearDupParams {
    /// Word *k*-shingles for space-delimited scripts (default 5).
    #[serde(default = "default_shingle_k")]
    pub shingle_k: usize,
    /// Character n-gram width for CJK runs (default 2).
    #[serde(default = "default_cjk_char_n")]
    pub cjk_char_n: usize,
    /// MinHash signature length H (default 128).
    #[serde(default = "default_num_hashes")]
    pub num_hashes: usize,
    /// LSH band count (default 16). Must satisfy `num_bands * rows_per_band == num_hashes`.
    #[serde(default = "default_num_bands")]
    pub num_bands: usize,
    /// Rows per LSH band (default 8).
    #[serde(default = "default_rows_per_band")]
    pub rows_per_band: usize,
    /// Minimum estimated Jaccard to link candidates (default 0.80).
    #[serde(default = "default_threshold")]
    pub threshold: f64,
    /// Fixed hash seed (default [`DEFAULT_HASH_SEED`]).
    #[serde(default = "default_hash_seed")]
    pub hash_seed: u64,
    /// Skip items with `dedup_role = duplicate` (default true).
    #[serde(default = "default_true")]
    pub skip_exact_duplicates: bool,
    /// Drop pure-digit word tokens (default true). Does not affect CJK n-grams.
    #[serde(default = "default_true")]
    pub ignore_numbers: bool,
    /// Minimum prepared char length; below → skipped (default 80).
    #[serde(default = "default_min_chars")]
    pub min_chars: usize,
    /// Clear prior near_dup result cols then recompute (default true).
    #[serde(default = "default_true")]
    pub reset: bool,
    /// Checkpoint / write batch size (default 200).
    #[serde(default = "default_batch_size")]
    pub batch_size: u64,
    /// Include attachment-role items with text (default true).
    #[serde(default = "default_true")]
    pub include_attachments: bool,
    /// Deep email quote stripping — **not implemented** in P0 (default false).
    #[serde(default)]
    pub strip_email_quotes: bool,
}

fn default_true() -> bool {
    true
}

fn default_shingle_k() -> usize {
    5
}

fn default_cjk_char_n() -> usize {
    2
}

fn default_num_hashes() -> usize {
    128
}

fn default_num_bands() -> usize {
    16
}

fn default_rows_per_band() -> usize {
    8
}

fn default_threshold() -> f64 {
    0.80
}

fn default_hash_seed() -> u64 {
    DEFAULT_HASH_SEED
}

fn default_min_chars() -> usize {
    80
}

fn default_batch_size() -> u64 {
    200
}

impl Default for NearDupParams {
    fn default() -> Self {
        Self {
            shingle_k: default_shingle_k(),
            cjk_char_n: default_cjk_char_n(),
            num_hashes: default_num_hashes(),
            num_bands: default_num_bands(),
            rows_per_band: default_rows_per_band(),
            threshold: default_threshold(),
            hash_seed: default_hash_seed(),
            skip_exact_duplicates: true,
            ignore_numbers: true,
            min_chars: default_min_chars(),
            reset: true,
            batch_size: default_batch_size(),
            include_attachments: true,
            strip_email_quotes: false,
        }
    }
}

impl NearDupParams {
    /// Parse from JSON, applying defaults for missing keys.
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        if json.trim().is_empty() {
            return Ok(Self::default());
        }
        serde_json::from_str(json)
    }

    /// Validate band geometry and positive sizes.
    pub fn validate(&self) -> Result<(), String> {
        if self.shingle_k == 0 {
            return Err("shingle_k must be >= 1".into());
        }
        if self.cjk_char_n == 0 {
            return Err("cjk_char_n must be >= 1".into());
        }
        if self.num_hashes == 0 {
            return Err("num_hashes must be >= 1".into());
        }
        if self.num_bands == 0 || self.rows_per_band == 0 {
            return Err("num_bands and rows_per_band must be >= 1".into());
        }
        if self.num_bands.saturating_mul(self.rows_per_band) != self.num_hashes {
            return Err(format!(
                "num_bands * rows_per_band must equal num_hashes ({} * {} != {})",
                self.num_bands, self.rows_per_band, self.num_hashes
            ));
        }
        if !(0.0..=1.0).contains(&self.threshold) {
            return Err("threshold must be in [0.0, 1.0]".into());
        }
        if self.batch_size == 0 {
            return Err("batch_size must be >= 1".into());
        }
        if self.strip_email_quotes {
            return Err(
                "strip_email_quotes is not implemented in minhash_shingle_v1 P0 (leave false)"
                    .into(),
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_from_empty_object() {
        let p = NearDupParams::from_json("{}").unwrap();
        assert_eq!(p.shingle_k, 5);
        assert_eq!(p.cjk_char_n, 2);
        assert_eq!(p.num_hashes, 128);
        assert_eq!(p.num_bands, 16);
        assert_eq!(p.rows_per_band, 8);
        assert!((p.threshold - 0.80).abs() < 1e-12);
        assert_eq!(p.hash_seed, DEFAULT_HASH_SEED);
        assert!(p.skip_exact_duplicates);
        assert!(p.ignore_numbers);
        assert_eq!(p.min_chars, 80);
        assert!(p.reset);
        assert_eq!(p.batch_size, 200);
        assert!(p.include_attachments);
        assert!(!p.strip_email_quotes);
        p.validate().unwrap();
    }

    #[test]
    fn rejects_bad_band_geometry() {
        let p = NearDupParams {
            num_bands: 10,
            rows_per_band: 8,
            num_hashes: 128,
            ..Default::default()
        };
        assert!(p.validate().is_err());
    }
}
