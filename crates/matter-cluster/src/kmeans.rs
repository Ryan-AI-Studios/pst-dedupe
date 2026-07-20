//! Deterministic k-means with empty-cluster drop and dense ordinals.

use crate::tfidf::SparseVec;

/// SplitMix64 PRNG — deterministic from seed.
#[derive(Debug, Clone)]
pub struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    pub fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9e3779b97f4a7c15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        z ^ (z >> 31)
    }

    pub fn next_usize(&mut self, upper: usize) -> usize {
        if upper == 0 {
            return 0;
        }
        (self.next_u64() as usize) % upper
    }
}

/// k-means result after empty drop + dense renumber.
#[derive(Debug, Clone)]
pub struct KMeansResult {
    /// Assignment of each document → dense cluster ordinal (0..cluster_count-1).
    pub assignment: Vec<usize>,
    /// Dense centroid vectors (vocab-dim).
    pub centroids: Vec<Vec<f64>>,
    /// Number of non-empty clusters.
    pub cluster_count: usize,
    /// Distance of each doc to its assigned centroid (optional quality).
    pub distances: Vec<f64>,
}

/// Run k-means on L2-normalized sparse rows.
///
/// - Requested `k` may exceed n_docs → clamped to n_docs (min 1 if n>0).
/// - Empty clusters dropped; ordinals dense 0..cluster_count-1.
pub fn kmeans(
    rows: &[SparseVec],
    vocab_dim: usize,
    k: usize,
    seed: u64,
    max_iters: u32,
) -> KMeansResult {
    let n = rows.len();
    if n == 0 || vocab_dim == 0 {
        return KMeansResult {
            assignment: Vec::new(),
            centroids: Vec::new(),
            cluster_count: 0,
            distances: Vec::new(),
        };
    }
    let k_eff = k.clamp(1, n);
    let mut rng = SplitMix64::new(seed);

    // Init: sample k distinct document indices (Fisher-Yates partial).
    let mut indices: Vec<usize> = (0..n).collect();
    for i in 0..k_eff {
        let j = i + rng.next_usize(n - i);
        indices.swap(i, j);
    }
    let mut centroids: Vec<Vec<f64>> = (0..k_eff)
        .map(|i| sparse_to_dense(&rows[indices[i]], vocab_dim))
        .collect();

    let mut assignment = vec![0usize; n];
    for _ in 0..max_iters {
        let mut changed = false;
        // Assign
        for (di, row) in rows.iter().enumerate() {
            let mut best = 0usize;
            let mut best_d = f64::INFINITY;
            for (ci, c) in centroids.iter().enumerate() {
                let d = row.dist_to_dense(c);
                if d < best_d || (d == best_d && ci < best) {
                    best_d = d;
                    best = ci;
                }
            }
            if assignment[di] != best {
                assignment[di] = best;
                changed = true;
            }
        }
        // Update centroids (empty → leave old; drop later)
        let mut sums = vec![vec![0.0; vocab_dim]; k_eff];
        let mut counts = vec![0u64; k_eff];
        for (di, row) in rows.iter().enumerate() {
            let c = assignment[di];
            counts[c] += 1;
            for (idx, val) in row.indices.iter().zip(row.values.iter()) {
                let i = *idx as usize;
                if i < vocab_dim {
                    sums[c][i] += *val;
                }
            }
        }
        for ci in 0..k_eff {
            if counts[ci] > 0 {
                let inv = 1.0 / counts[ci] as f64;
                for v in &mut sums[ci] {
                    *v *= inv;
                }
                // L2-normalize centroid for spherical geometry consistency.
                let norm = sums[ci].iter().map(|v| v * v).sum::<f64>().sqrt();
                if norm > 0.0 {
                    for v in &mut sums[ci] {
                        *v /= norm;
                    }
                }
                centroids[ci] = sums[ci].clone();
            }
        }
        if !changed {
            break;
        }
    }

    // Drop empty clusters; dense renumber.
    let mut nonempty: Vec<usize> = (0..k_eff).filter(|&ci| assignment.contains(&ci)).collect();
    nonempty.sort_unstable();
    let remap: Vec<Option<usize>> = {
        let mut m = vec![None; k_eff];
        for (new_i, &old) in nonempty.iter().enumerate() {
            m[old] = Some(new_i);
        }
        m
    };
    let cluster_count = nonempty.len();
    let new_centroids: Vec<Vec<f64>> = nonempty.iter().map(|&i| centroids[i].clone()).collect();
    let mut new_assign = vec![0usize; n];
    let mut distances = vec![0.0; n];
    for (di, row) in rows.iter().enumerate() {
        let old = assignment[di];
        let new_c = remap[old].unwrap_or(0);
        new_assign[di] = new_c;
        if new_c < new_centroids.len() {
            distances[di] = row.dist_to_dense(&new_centroids[new_c]);
        }
    }

    KMeansResult {
        assignment: new_assign,
        centroids: new_centroids,
        cluster_count,
        distances,
    }
}

fn sparse_to_dense(v: &SparseVec, dim: usize) -> Vec<f64> {
    let mut d = vec![0.0; dim];
    for (idx, val) in v.indices.iter().zip(v.values.iter()) {
        let i = *idx as usize;
        if i < dim {
            d[i] = *val;
        }
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drop_empty_dense_ordinals() {
        // Two well-separated unit vectors + force k=3 so one empty centroid is dropped.
        let a = SparseVec {
            indices: vec![0],
            values: vec![1.0],
        };
        let b = SparseVec {
            indices: vec![1],
            values: vec![1.0],
        };
        // Duplicate a so cluster of a is nonempty.
        let rows = vec![a.clone(), a, b.clone(), b];
        let r = kmeans(&rows, 2, 3, 1, 20);
        // Requested k=3 with only two natural topics → empty cluster dropped.
        assert!(
            r.cluster_count < 3,
            "expected empty drop: cluster_count={} (k=3)",
            r.cluster_count
        );
        assert!(r.cluster_count >= 1);
        assert!(r.assignment.iter().all(|&c| c < r.cluster_count));
        // Dense: max ordinal == cluster_count-1
        let max = *r.assignment.iter().max().unwrap();
        assert_eq!(max, r.cluster_count - 1);
        // Every assignment references a written ordinal (0..count-1).
        for &c in &r.assignment {
            assert!(c < r.cluster_count);
        }
    }

    #[test]
    fn determinism_same_seed() {
        let rows: Vec<SparseVec> = (0..10)
            .map(|i| SparseVec {
                indices: vec![(i % 3) as u32, ((i + 1) % 3) as u32],
                values: vec![0.8, 0.6],
            })
            .map(|mut v| {
                v.l2_normalize();
                v
            })
            .collect();
        let r1 = kmeans(&rows, 3, 2, 99, 30);
        let r2 = kmeans(&rows, 3, 2, 99, 30);
        assert_eq!(r1.assignment, r2.assignment);
        assert_eq!(r1.cluster_count, r2.cluster_count);
    }
}
