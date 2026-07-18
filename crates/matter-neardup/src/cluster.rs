//! Union-find clustering, pivot selection, and weak-member demotion.

use std::collections::HashMap;

use crate::minhash::MinHashSig;

/// Disjoint-set over dense indices (`0..n`).
#[derive(Debug)]
pub struct UnionFind {
    parent: Vec<usize>,
    rank: Vec<u8>,
}

impl UnionFind {
    pub fn new(n: usize) -> Self {
        Self {
            parent: (0..n).collect(),
            rank: vec![0; n],
        }
    }

    pub fn find(&mut self, x: usize) -> usize {
        let mut x = x;
        while self.parent[x] != x {
            self.parent[x] = self.parent[self.parent[x]];
            x = self.parent[x];
        }
        x
    }

    pub fn union(&mut self, a: usize, b: usize) {
        let mut ra = self.find(a);
        let mut rb = self.find(b);
        if ra == rb {
            return;
        }
        if self.rank[ra] < self.rank[rb] {
            std::mem::swap(&mut ra, &mut rb);
        }
        self.parent[rb] = ra;
        if self.rank[ra] == self.rank[rb] {
            self.rank[ra] += 1;
        }
    }
}

/// Per-item meta used for pivot ties (stable order keys).
#[derive(Debug, Clone)]
pub struct ItemMeta {
    pub item_id: String,
    pub token_count: usize,
    pub imported_at: String,
    pub path: String,
    pub sig: MinHashSig,
}

/// Final assignment after clustering + demotion.
#[derive(Debug, Clone)]
pub struct ClusterAssignment {
    pub item_id: String,
    /// `pivot` | `member` | `unique`
    pub role: String,
    pub group_id: Option<String>,
    pub pivot_item_id: Option<String>,
    pub similarity: Option<f64>,
}

/// Group id = full SHA-256 hex of `near:v1\n{pivot_item_id}`.
pub fn near_group_id(pivot_item_id: &str) -> String {
    use sha2::{Digest, Sha256};
    let preimage = format!("near:v1\n{pivot_item_id}");
    let digest = Sha256::digest(preimage.as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// Pivot = max token_count; ties → imported_at ASC, path ASC, id ASC.
fn pick_pivot(items: &[ItemMeta], members: &[usize]) -> usize {
    *members
        .iter()
        .min_by(|&&a, &&b| {
            items[b]
                .token_count
                .cmp(&items[a].token_count)
                .then_with(|| items[a].imported_at.cmp(&items[b].imported_at))
                .then_with(|| items[a].path.cmp(&items[b].path))
                .then_with(|| items[a].item_id.cmp(&items[b].item_id))
        })
        .expect("non-empty component")
}

/// Link candidate pairs at ≥ threshold, pick pivots, demote weak members.
pub fn cluster_and_score(
    items: &[ItemMeta],
    pairs: &[(usize, usize)],
    threshold: f64,
) -> Vec<ClusterAssignment> {
    let n = items.len();
    let mut uf = UnionFind::new(n);
    for &(a, b) in pairs {
        if a >= n || b >= n {
            continue;
        }
        let sim = items[a].sig.estimate_jaccard(&items[b].sig);
        if sim >= threshold {
            uf.union(a, b);
        }
    }

    let mut components: HashMap<usize, Vec<usize>> = HashMap::new();
    for i in 0..n {
        let root = uf.find(i);
        components.entry(root).or_default().push(i);
    }

    let mut out = Vec::with_capacity(n);
    let mut assigned = vec![false; n];

    for members in components.values() {
        if members.len() < 2 {
            let i = members[0];
            assigned[i] = true;
            out.push(ClusterAssignment {
                item_id: items[i].item_id.clone(),
                role: "unique".into(),
                group_id: None,
                pivot_item_id: None,
                similarity: None,
            });
            continue;
        }

        let pivot_idx = pick_pivot(items, members);
        let pivot = &items[pivot_idx];
        let gid = near_group_id(&pivot.item_id);

        let mut kept = vec![pivot_idx];
        let mut demoted = Vec::new();

        for &i in members {
            if i == pivot_idx {
                continue;
            }
            let sim = items[i].sig.estimate_jaccard(&pivot.sig);
            if sim >= threshold {
                kept.push(i);
            } else {
                demoted.push(i);
            }
        }

        if kept.len() < 2 {
            // Everyone demoted (or only pivot left) → all unique
            for &i in members {
                assigned[i] = true;
                out.push(ClusterAssignment {
                    item_id: items[i].item_id.clone(),
                    role: "unique".into(),
                    group_id: None,
                    pivot_item_id: None,
                    similarity: None,
                });
            }
            continue;
        }

        for &i in &kept {
            assigned[i] = true;
            if i == pivot_idx {
                out.push(ClusterAssignment {
                    item_id: items[i].item_id.clone(),
                    role: "pivot".into(),
                    group_id: Some(gid.clone()),
                    pivot_item_id: Some(pivot.item_id.clone()),
                    similarity: Some(1.0),
                });
            } else {
                let sim = items[i].sig.estimate_jaccard(&pivot.sig);
                out.push(ClusterAssignment {
                    item_id: items[i].item_id.clone(),
                    role: "member".into(),
                    group_id: Some(gid.clone()),
                    pivot_item_id: Some(pivot.item_id.clone()),
                    similarity: Some(sim),
                });
            }
        }
        for &i in &demoted {
            assigned[i] = true;
            out.push(ClusterAssignment {
                item_id: items[i].item_id.clone(),
                role: "unique".into(),
                group_id: None,
                pivot_item_id: None,
                similarity: None,
            });
        }
    }

    for i in 0..n {
        if !assigned[i] {
            out.push(ClusterAssignment {
                item_id: items[i].item_id.clone(),
                role: "unique".into(),
                group_id: None,
                pivot_item_id: None,
                similarity: None,
            });
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::minhash::minhash_signature;
    use std::collections::BTreeSet;

    fn meta(id: &str, tokens: usize, path: &str, shingles: &[&str]) -> ItemMeta {
        let mut set = BTreeSet::new();
        for s in shingles {
            set.insert((*s).to_string());
        }
        ItemMeta {
            item_id: id.into(),
            token_count: tokens,
            imported_at: "2020-01-01T00:00:00Z".into(),
            path: path.into(),
            sig: minhash_signature(&set, 1, 64),
        }
    }

    #[test]
    fn pivot_prefers_higher_token_count() {
        let items = vec![
            meta("a", 10, "a", &["x1", "x2", "x3", "x4", "x5"]),
            meta("b", 20, "b", &["x1", "x2", "x3", "x4", "x5"]),
        ];
        // Force same sig by using same shingles — both will link
        let pairs = vec![(0, 1)];
        let out = cluster_and_score(&items, &pairs, 0.5);
        let pivot = out.iter().find(|a| a.role == "pivot").unwrap();
        assert_eq!(pivot.item_id, "b");
    }
}
