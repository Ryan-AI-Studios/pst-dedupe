//! Banded LSH candidate generation over MinHash signatures.

use std::collections::{HashMap, HashSet};

use crate::minhash::MinHashSig;

/// Generate unordered candidate pairs via banded LSH.
///
/// For each band `b` in `0..num_bands`, the band key is the `rows_per_band`
/// consecutive slot values starting at `b * rows_per_band`. Items sharing a
/// band key are pairwise candidates.
pub fn lsh_candidate_pairs(
    sigs: &[(usize, &MinHashSig)],
    num_bands: usize,
    rows_per_band: usize,
) -> HashSet<(usize, usize)> {
    let mut pairs = HashSet::new();
    if num_bands == 0 || rows_per_band == 0 || sigs.is_empty() {
        return pairs;
    }
    for b in 0..num_bands {
        let start = b * rows_per_band;
        let end = start + rows_per_band;
        let mut buckets: HashMap<Vec<u64>, Vec<usize>> = HashMap::new();
        for &(idx, sig) in sigs {
            if end > sig.slots.len() {
                continue;
            }
            let key = sig.slots[start..end].to_vec();
            buckets.entry(key).or_default().push(idx);
        }
        for members in buckets.values() {
            if members.len() < 2 {
                continue;
            }
            for i in 0..members.len() {
                for j in (i + 1)..members.len() {
                    let a = members[i];
                    let b = members[j];
                    if a < b {
                        pairs.insert((a, b));
                    } else {
                        pairs.insert((b, a));
                    }
                }
            }
        }
    }
    pairs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::minhash::{minhash_signature, MinHashSig};
    use std::collections::BTreeSet;

    #[test]
    fn identical_sigs_are_candidates() {
        let mut set = BTreeSet::new();
        for i in 0..30 {
            set.insert(format!("tok-{i}"));
        }
        let s = minhash_signature(&set, 1, 128);
        let sigs: Vec<(usize, &MinHashSig)> = vec![(0, &s), (1, &s)];
        let pairs = lsh_candidate_pairs(&sigs, 16, 8);
        assert!(pairs.contains(&(0, 1)));
    }
}
