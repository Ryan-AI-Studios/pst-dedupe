//! Union-find over compact Message-ID keys (header graph).

use std::collections::HashMap;

use crate::keys::CompactKey;

/// Disjoint-set (union-find) keyed by [`CompactKey`].
#[derive(Debug, Default)]
pub struct UnionFind {
    parent: HashMap<CompactKey, CompactKey>,
    rank: HashMap<CompactKey, u8>,
}

impl UnionFind {
    pub fn new() -> Self {
        Self::default()
    }

    /// Ensure `k` is a member (singleton component if new).
    pub fn make_set(&mut self, k: CompactKey) {
        self.parent.entry(k).or_insert(k);
        self.rank.entry(k).or_insert(0);
    }

    pub fn find(&mut self, k: CompactKey) -> CompactKey {
        self.make_set(k);
        let p = self.parent[&k];
        if p != k {
            let root = self.find(p);
            self.parent.insert(k, root);
            root
        } else {
            k
        }
    }

    pub fn union(&mut self, a: CompactKey, b: CompactKey) {
        let ra = self.find(a);
        let rb = self.find(b);
        if ra == rb {
            return;
        }
        let rank_a = self.rank[&ra];
        let rank_b = self.rank[&rb];
        if rank_a < rank_b {
            self.parent.insert(ra, rb);
        } else if rank_a > rank_b {
            self.parent.insert(rb, ra);
        } else {
            self.parent.insert(rb, ra);
            self.rank.insert(ra, rank_a + 1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unions_connect() {
        let mut uf = UnionFind::new();
        let a = [1u8; 32];
        let b = [2u8; 32];
        let c = [3u8; 32];
        uf.union(a, b);
        uf.union(b, c);
        assert_eq!(uf.find(a), uf.find(c));
        let d = [4u8; 32];
        uf.make_set(d);
        assert_ne!(uf.find(a), uf.find(d));
    }
}
