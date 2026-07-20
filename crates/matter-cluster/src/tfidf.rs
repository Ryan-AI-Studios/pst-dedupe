//! Sparse TF–IDF with DF filters and **mandatory L2** row normalization.

use std::collections::BTreeMap;

/// Sparse document vector: sorted (term_index, weight) pairs.
#[derive(Debug, Clone, Default)]
pub struct SparseVec {
    pub indices: Vec<u32>,
    pub values: Vec<f64>,
}

impl SparseVec {
    pub fn is_zero(&self) -> bool {
        self.values.iter().all(|v| *v == 0.0) || self.values.is_empty()
    }

    pub fn l2_norm(&self) -> f64 {
        self.values.iter().map(|v| v * v).sum::<f64>().sqrt()
    }

    /// In-place L2 normalize. Zero vector stays zero (documented policy).
    pub fn l2_normalize(&mut self) {
        let n = self.l2_norm();
        if n > 0.0 {
            for v in &mut self.values {
                *v /= n;
            }
        }
    }

    pub fn dot(&self, other: &SparseVec) -> f64 {
        let mut i = 0usize;
        let mut j = 0usize;
        let mut sum = 0.0;
        while i < self.indices.len() && j < other.indices.len() {
            match self.indices[i].cmp(&other.indices[j]) {
                std::cmp::Ordering::Equal => {
                    sum += self.values[i] * other.values[j];
                    i += 1;
                    j += 1;
                }
                std::cmp::Ordering::Less => i += 1,
                std::cmp::Ordering::Greater => j += 1,
            }
        }
        sum
    }

    /// Euclidean distance to dense centroid (centroid aligned to full vocab).
    pub fn dist_to_dense(&self, centroid: &[f64]) -> f64 {
        let mut sum = 0.0;
        let mut seen = vec![false; centroid.len()];
        for (idx, val) in self.indices.iter().zip(self.values.iter()) {
            let i = *idx as usize;
            if i < centroid.len() {
                let d = val - centroid[i];
                sum += d * d;
                seen[i] = true;
            }
        }
        for (i, c) in centroid.iter().enumerate() {
            if !seen[i] && *c != 0.0 {
                sum += c * c;
            }
        }
        sum.sqrt()
    }
}

/// Vocabulary: term → index.
#[derive(Debug, Clone, Default)]
pub struct Vocabulary {
    pub terms: Vec<String>,
    pub index: BTreeMap<String, u32>,
    pub idf: Vec<f64>,
}

/// Build vocab from per-doc term counts with DF filters and max_vocab.
pub fn build_vocabulary(
    docs: &[BTreeMap<String, u32>],
    min_df: u32,
    max_df_ratio: f64,
    max_vocab: u32,
) -> Vocabulary {
    let n_docs = docs.len() as f64;
    if n_docs == 0.0 {
        return Vocabulary::default();
    }
    let mut df: BTreeMap<String, u32> = BTreeMap::new();
    for doc in docs {
        for t in doc.keys() {
            *df.entry(t.clone()).or_insert(0) += 1;
        }
    }
    let max_df = (max_df_ratio * n_docs).floor().max(1.0) as u32;
    // Collect eligible terms with DF, sort by DF desc then term asc, take max_vocab.
    let mut eligible: Vec<(String, u32)> = df
        .into_iter()
        .filter(|(_, d)| *d >= min_df && *d <= max_df)
        .collect();
    eligible.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    if eligible.len() > max_vocab as usize {
        eligible.truncate(max_vocab as usize);
    }
    // Stable final order: alphabetical for deterministic indices.
    eligible.sort_by(|a, b| a.0.cmp(&b.0));

    let mut terms = Vec::with_capacity(eligible.len());
    let mut index = BTreeMap::new();
    let mut idf = Vec::with_capacity(eligible.len());
    for (i, (term, d)) in eligible.into_iter().enumerate() {
        // smooth IDF
        let idf_t = (1.0 + n_docs / (1.0 + f64::from(d))).ln();
        index.insert(term.clone(), i as u32);
        terms.push(term);
        idf.push(idf_t);
    }
    Vocabulary { terms, index, idf }
}

/// Build sparse TF–IDF vector (log TF) then **mandatory L2**.
pub fn doc_to_tfidf(counts: &BTreeMap<String, u32>, vocab: &Vocabulary) -> SparseVec {
    let mut pairs: Vec<(u32, f64)> = Vec::new();
    for (term, &tf) in counts {
        if let Some(&idx) = vocab.index.get(term) {
            let log_tf = 1.0 + (f64::from(tf)).ln();
            let w = log_tf * vocab.idf[idx as usize];
            if w != 0.0 {
                pairs.push((idx, w));
            }
        }
    }
    pairs.sort_by_key(|(i, _)| *i);
    let mut vec = SparseVec {
        indices: pairs.iter().map(|(i, _)| *i).collect(),
        values: pairs.iter().map(|(_, v)| *v).collect(),
    };
    // Mandatory L2 — zero vector stays zero.
    vec.l2_normalize();
    vec
}

/// Build all document vectors.
pub fn build_matrix(docs: &[BTreeMap<String, u32>], vocab: &Vocabulary) -> Vec<SparseVec> {
    docs.iter().map(|d| doc_to_tfidf(d, vocab)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn l2_unit_norm() {
        let mut v = SparseVec {
            indices: vec![0, 1],
            values: vec![3.0, 4.0],
        };
        v.l2_normalize();
        assert!((v.l2_norm() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn zero_stays_zero() {
        let mut v = SparseVec::default();
        v.l2_normalize();
        assert!(v.is_zero());
    }

    #[test]
    fn length_bias_removed_after_l2() {
        // Same terms, different magnitudes → same L2 direction.
        let vocab = Vocabulary {
            terms: vec!["invoice".into(), "payment".into()],
            index: [("invoice".into(), 0), ("payment".into(), 1)]
                .into_iter()
                .collect(),
            idf: vec![1.0, 1.0],
        };
        let short: BTreeMap<_, _> = [("invoice".into(), 1), ("payment".into(), 1)]
            .into_iter()
            .collect();
        let long: BTreeMap<_, _> = [("invoice".into(), 50), ("payment".into(), 50)]
            .into_iter()
            .collect();
        let vs = doc_to_tfidf(&short, &vocab);
        let vl = doc_to_tfidf(&long, &vocab);
        // Cosine ≈ 1 after L2 on same support.
        let cos = vs.dot(&vl);
        assert!(cos > 0.99, "cos={cos}");
    }
}
