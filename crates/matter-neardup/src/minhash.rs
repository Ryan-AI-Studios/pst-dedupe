//! MinHash signatures via seeded SplitMix64 stream (Approach A).
//!
//! # Recipe for `minhash_shingle_v1` (frozen)
//!
//! For each unique shingle string `S` (UTF-8):
//!
//! 1. `digest = SHA-256(S_utf8)`
//! 2. `first_u64 = u64::from_be_bytes(digest[0..8])`
//! 3. `base = first_u64 XOR hash_seed`
//! 4. Seed an in-crate **SplitMix64** PRNG with `base`
//! 5. Emit the next `H` `u64` values as the hash images of this shingle
//! 6. MinHash slot `i` = minimum over all shingles of stream value `i`
//!
//! ## Forbidden
//!
//! Kirsch–Mitzenmacher double-hash form `h1.wrapping_add(i.wrapping_mul(h2))`
//! derived from a single digest pair — **not** used here.
//!
//! ## SplitMix64 constants (Steele / Vigna; hard-coded, no `rand` crate)
//!
//! - gamma / increment: `0x9E3779B97F4A7C15`
//! - mix mul 1: `0xBF58476D1CE4E5B9`
//! - mix mul 2: `0x94D049BB133111EB`

use std::collections::BTreeSet;

use sha2::{Digest, Sha256};

/// In-crate SplitMix64 (64-bit state). Fully deterministic; no OS RNG.
#[derive(Debug, Clone, Copy)]
pub struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    pub fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    /// Next pseudo-random `u64` (SplitMix64).
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E3779B97F4A7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        z ^ (z >> 31)
    }
}

/// Fixed-size MinHash signature (`H` slots of `u64`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MinHashSig {
    pub slots: Vec<u64>,
}

impl MinHashSig {
    pub fn num_hashes(&self) -> usize {
        self.slots.len()
    }

    /// Estimated Jaccard similarity = fraction of equal slots.
    pub fn estimate_jaccard(&self, other: &MinHashSig) -> f64 {
        assert_eq!(self.slots.len(), other.slots.len());
        if self.slots.is_empty() {
            return 0.0;
        }
        let eq = self
            .slots
            .iter()
            .zip(other.slots.iter())
            .filter(|(a, b)| a == b)
            .count();
        eq as f64 / self.slots.len() as f64
    }
}

/// First 8 bytes of SHA-256 as big-endian `u64`.
pub fn first_u64_sha256(bytes: &[u8]) -> u64 {
    let digest = Sha256::digest(bytes);
    let mut arr = [0u8; 8];
    arr.copy_from_slice(&digest[..8]);
    u64::from_be_bytes(arr)
}

/// Expand one shingle into `H` independent-looking hash images (Approach A).
pub fn expand_shingle_hashes(shingle_utf8: &[u8], hash_seed: u64, h: usize) -> Vec<u64> {
    let base = first_u64_sha256(shingle_utf8) ^ hash_seed;
    let mut rng = SplitMix64::new(base);
    let mut out = Vec::with_capacity(h);
    for _ in 0..h {
        out.push(rng.next_u64());
    }
    out
}

/// Compute MinHash signature over a unique shingle set.
pub fn minhash_signature(shingles: &BTreeSet<String>, hash_seed: u64, h: usize) -> MinHashSig {
    let mut slots = vec![u64::MAX; h];
    if shingles.is_empty() || h == 0 {
        return MinHashSig { slots };
    }
    for s in shingles {
        let images = expand_shingle_hashes(s.as_bytes(), hash_seed, h);
        for i in 0..h {
            if images[i] < slots[i] {
                slots[i] = images[i];
            }
        }
    }
    MinHashSig { slots }
}

/// Kirsch–Mitzenmacher expansion (FORBIDDEN for this method) — used only in tests
/// to prove we do **not** match it.
#[cfg(test)]
pub fn km_double_hash_images(shingle_utf8: &[u8], h: usize) -> Vec<u64> {
    let digest = Sha256::digest(shingle_utf8);
    let mut a = [0u8; 8];
    let mut b = [0u8; 8];
    a.copy_from_slice(&digest[0..8]);
    b.copy_from_slice(&digest[8..16]);
    let h1 = u64::from_be_bytes(a);
    let h2 = u64::from_be_bytes(b) | 1; // odd step often used
    (0..h as u64)
        .map(|i| h1.wrapping_add(i.wrapping_mul(h2)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splitmix_golden_sequence() {
        // Fixed seed → hard-coded external golden vector (algorithm freeze).
        // Values are absolute literals, not recomputed from the same mix steps.
        let mut rng = SplitMix64::new(0x1234_5678_9ABC_DEF0);
        let got: Vec<u64> = (0..8).map(|_| rng.next_u64()).collect();
        const EXPECTED: [u64; 8] = [
            0x1619_22c6_45ce_50e8,
            0xad76_0caf_a169_7b60,
            0x3501_ff44_902c_a50d,
            0x417c_b9a8_26d8_31df,
            0x99af_6f9b_0c44_76b6,
            0x5d51_f5f7_5b76_2c59,
            0x6623_9e8c_309a_282b,
            0x53e0_1f58_0916_c5cb,
        ];
        assert_eq!(got, EXPECTED);
        // Non-zero / not identity
        assert_ne!(got[0], 0);
        assert_ne!(got[0], got[1]);
    }

    #[test]
    fn expansion_is_not_km_double_hash() {
        let shingle = b"the\x1fquick\x1fbrown\x1ffox\x1fjumps";
        let seed = 0x4E44_5F6D_685F_7631u64;
        let h = 16;
        let ours = expand_shingle_hashes(shingle, seed, h);
        let km = km_double_hash_images(shingle, h);
        assert_ne!(
            ours, km,
            "Approach A must not match Kirsch–Mitzenmacher h1+i*h2"
        );
        // Also ensure slots are not arithmetic progression of a fixed step
        // (KM with constant h2 produces linear congruential sequence).
        let step = ours[1].wrapping_sub(ours[0]);
        let looks_like_km =
            (2..h).all(|i| ours[i] == ours[0].wrapping_add((i as u64).wrapping_mul(step)));
        assert!(
            !looks_like_km,
            "expansion must not be pure linear h1+i*step"
        );
    }

    #[test]
    fn identical_shingle_sets_jaccard_one() {
        let mut a = BTreeSet::new();
        a.insert("alpha".into());
        a.insert("beta".into());
        a.insert("gamma".into());
        let sa = minhash_signature(&a, 42, 64);
        let sb = minhash_signature(&a, 42, 64);
        assert!((sa.estimate_jaccard(&sb) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn disjoint_sets_low_jaccard() {
        let mut a = BTreeSet::new();
        let mut b = BTreeSet::new();
        for i in 0..40 {
            a.insert(format!("alpha-token-{i}"));
            b.insert(format!("omega-token-{i}"));
        }
        let sa = minhash_signature(&a, 42, 128);
        let sb = minhash_signature(&b, 42, 128);
        assert!(sa.estimate_jaccard(&sb) < 0.3);
    }
}
