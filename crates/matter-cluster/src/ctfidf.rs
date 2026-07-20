//! True c-TF-IDF labels with inverse cluster frequency (ICF).

use std::collections::{BTreeMap, BTreeSet};

/// Compute label terms for each cluster via c-TF-IDF / ICF.
///
/// - Cluster bag = sum of term counts over member docs
/// - `tf` = log TF of bag
/// - `icf_t = ln(1 + N_c / df_cluster_t)`
/// - score = tf * icf
/// - top `label_terms` → ordered list + joined label
pub fn cluster_labels(
    docs: &[BTreeMap<String, u32>],
    assignment: &[usize],
    cluster_count: usize,
    label_terms: usize,
) -> Vec<(String, Vec<String>)> {
    if cluster_count == 0 {
        return Vec::new();
    }
    // Build cluster bags.
    let mut bags: Vec<BTreeMap<String, u32>> = vec![BTreeMap::new(); cluster_count];
    for (di, &c) in assignment.iter().enumerate() {
        if c >= cluster_count || di >= docs.len() {
            continue;
        }
        for (term, &cnt) in &docs[di] {
            *bags[c].entry(term.clone()).or_insert(0) += cnt;
        }
    }
    // Cluster DF: how many clusters contain term t.
    let mut cluster_df: BTreeMap<String, u32> = BTreeMap::new();
    for bag in &bags {
        for t in bag.keys() {
            *cluster_df.entry(t.clone()).or_insert(0) += 1;
        }
    }
    let n_c = cluster_count as f64;
    let mut out = Vec::with_capacity(cluster_count);
    for bag in &bags {
        let mut scored: Vec<(String, f64)> = bag
            .iter()
            .map(|(term, &tf)| {
                let log_tf = 1.0 + (f64::from(tf)).ln();
                let df_c = f64::from(*cluster_df.get(term).unwrap_or(&1));
                let icf = (1.0 + n_c / df_c).ln();
                (term.clone(), log_tf * icf)
            })
            .collect();
        scored.sort_by(|a, b| {
            b.1.partial_cmp(&a.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.0.cmp(&b.0))
        });
        scored.truncate(label_terms.max(1));
        let terms: Vec<String> = scored.into_iter().map(|(t, _)| t).collect();
        let label = if terms.is_empty() {
            "cluster".to_string()
        } else {
            terms.join(" ")
        };
        out.push((label, terms));
    }
    out
}

/// Jaccard of two top-term sets (for tests).
pub fn jaccard(a: &[String], b: &[String]) -> f64 {
    let sa: BTreeSet<_> = a.iter().collect();
    let sb: BTreeSet<_> = b.iter().collect();
    let inter = sa.intersection(&sb).count() as f64;
    let union = sa.union(&sb).count() as f64;
    if union == 0.0 {
        0.0
    } else {
        inter / union
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distinct_labels_for_disjoint_themes() {
        let d0: BTreeMap<_, _> = [
            ("invoice".into(), 5),
            ("payment".into(), 4),
            ("vendor".into(), 3),
        ]
        .into_iter()
        .collect();
        let d1: BTreeMap<_, _> = [
            ("invoice".into(), 1),
            ("payment".into(), 1),
            ("vendor".into(), 1),
        ]
        .into_iter()
        .collect();
        let d2: BTreeMap<_, _> = [
            ("patient".into(), 5),
            ("clinical".into(), 4),
            ("dosage".into(), 3),
        ]
        .into_iter()
        .collect();
        let d3: BTreeMap<_, _> = [
            ("patient".into(), 2),
            ("clinical".into(), 2),
            ("dosage".into(), 2),
        ]
        .into_iter()
        .collect();
        let docs = vec![d0, d1, d2, d3];
        let assignment = vec![0, 0, 1, 1];
        let labels = cluster_labels(&docs, &assignment, 2, 5);
        assert_eq!(labels.len(), 2);
        // Top-1 should differ for distinct themes.
        assert_ne!(
            labels[0].1.first(),
            labels[1].1.first(),
            "labels={labels:?}"
        );
        let j = jaccard(&labels[0].1, &labels[1].1);
        assert!(j < 1.0, "jaccard={j}");
    }
}
