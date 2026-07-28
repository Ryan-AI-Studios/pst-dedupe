//! In-memory dedup index — HashMap-based with tiered lookup (0076 hardened).

use std::collections::{HashMap, HashSet};

use crate::grouping::{
    mid_join_compatible, mid_present, BoundBy, DedupeScope, GroupingContext, GroupingStats,
    IdentityLevel,
};

/// Which dedup tier matched (legacy enum; prefer [`BoundBy`] for provenance).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DedupTier {
    /// Tier 1: Message-ID exact match.
    MessageId,
    /// Tier 2: SHA-256 content hash match.
    ContentHash,
    /// Tier 2.5: strong content hash match.
    StrongContentHash,
}

impl std::fmt::Display for DedupTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DedupTier::MessageId => write!(f, "Message-ID"),
            DedupTier::ContentHash => write!(f, "Content Hash"),
            DedupTier::StrongContentHash => write!(f, "Strong Content Hash"),
        }
    }
}

impl From<BoundBy> for Option<DedupTier> {
    fn from(b: BoundBy) -> Self {
        match b {
            BoundBy::Seed => None,
            BoundBy::MessageId => Some(DedupTier::MessageId),
            BoundBy::ContentHash => Some(DedupTier::ContentHash),
            BoundBy::StrongContentHash => Some(DedupTier::StrongContentHash),
        }
    }
}

/// Reference to a specific message in a specific PST file.
#[derive(Debug, Clone)]
pub struct MessageRef {
    /// Index into the input PST file list.
    pub pst_index: usize,
    /// PST filename.
    pub pst_name: String,
    /// Folder path within the PST.
    pub folder_path: String,
    /// Message NID (for re-extraction if needed).
    pub nid: u64,
    /// Subject line.
    pub subject: String,
    /// Submit time as FILETIME.
    pub submit_time: Option<i64>,
    /// Sender email address.
    pub sender: String,
    /// Message size in bytes.
    pub size: u32,
}

/// Result of checking a message against the index.
#[derive(Debug, Clone)]
pub enum DedupResult {
    /// First occurrence — this message is unique (so far). Bound as [`BoundBy::Seed`].
    Unique,
    /// Duplicate of an earlier message.
    DuplicateOf {
        /// The original (first-seen) message.
        original: MessageRef,
        /// Which tier detected the match (legacy).
        tier: DedupTier,
        /// Bind provenance recorded at match time.
        bound_by: BoundBy,
    },
}

impl DedupResult {
    pub fn is_unique(&self) -> bool {
        matches!(self, Self::Unique)
    }

    pub fn bound_by(&self) -> BoundBy {
        match self {
            Self::Unique => BoundBy::Seed,
            Self::DuplicateOf { bound_by, .. } => *bound_by,
        }
    }
}

/// Input for a single index check/insert (0076).
#[derive(Debug, Clone)]
pub struct IndexItem {
    pub message_id: Option<String>,
    /// v1 content hash (always present).
    pub content_hash: [u8; 32],
    /// Strong hash when identity level ≥ body.
    pub strong_content_hash: Option<[u8; 32]>,
    /// Whether this item may bind/insert on the content-hash map.
    pub tier2_eligible: bool,
    /// Source path compare key for per-source scope (lowercased absolute on Windows).
    pub source_key: String,
    /// Body component fingerprint (for tier1_verify body / divergence).
    pub fp_body: u64,
    pub fp_header: u64,
    pub fp_recipients: u64,
    pub fp_attachments: u64,
    pub msg_ref: MessageRef,
}

impl IndexItem {
    /// Minimal constructor from classic check_and_insert args (eligible, global).
    pub fn classic(message_id: Option<&str>, content_hash: [u8; 32], msg_ref: MessageRef) -> Self {
        Self {
            message_id: message_id.map(|s| s.to_string()),
            content_hash,
            strong_content_hash: None,
            tier2_eligible: true,
            source_key: String::new(),
            fp_body: 0,
            fp_header: 0,
            fp_recipients: 0,
            fp_attachments: 0,
            msg_ref,
        }
    }
}

struct GroupSlot {
    msg_ref: MessageRef,
    bound_mid: Option<String>,
    /// First-seen content/strong hash for this group (for divergence).
    bind_hash: [u8; 32],
    fp_body: u64,
    fp_header: u64,
    fp_recipients: u64,
    fp_attachments: u64,
}

/// The dedup index. Insert messages in scan order; check returns Unique or DuplicateOf.
pub struct DedupIndex {
    ctx: GroupingContext,
    /// Tier 1: scope||mid → group slot.
    message_ids: HashMap<String, GroupSlot>,
    /// Tier 2: scope||hash → group slot.
    content_hashes: HashMap<Vec<u8>, GroupSlot>,
    /// Running counts.
    pub unique_count: u64,
    pub duplicate_count: u64,
    pub tier1_hits: u64,
    pub tier2_hits: u64,
    /// 0076 honesty counters.
    pub stats: GroupingStats,
    /// Content hashes that saw at least one cross-MID block (for _groups count).
    cross_mid_hash_seen: HashSet<Vec<u8>>,
    /// Per content-hash: count of distinct MIDs that collided (for max_group).
    cross_mid_cluster: HashMap<Vec<u8>, HashSet<String>>,
}

impl DedupIndex {
    pub fn new() -> Self {
        Self::with_context(GroupingContext::default())
    }

    /// With pre-allocated capacity for expected message count.
    pub fn with_capacity(expected: usize) -> Self {
        Self::with_capacity_and_context(expected, GroupingContext::default())
    }

    /// Create with Tier 2 explicitly enabled or disabled (0076 default guards).
    pub fn with_tier2(enabled: bool) -> Self {
        Self::with_context(GroupingContext::with_tier2(enabled))
    }

    /// Create with capacity and Tier 2 setting.
    pub fn with_capacity_and_tier2(expected: usize, tier2_enabled: bool) -> Self {
        Self::with_capacity_and_context(expected, GroupingContext::with_tier2(tier2_enabled))
    }

    /// Full context constructor.
    pub fn with_context(ctx: GroupingContext) -> Self {
        Self::with_capacity_and_context(0, ctx)
    }

    pub fn with_capacity_and_context(expected: usize, ctx: GroupingContext) -> Self {
        let cap = expected.max(1);
        Self {
            content_hashes: if ctx.tier2_enabled {
                HashMap::with_capacity(cap / 4)
            } else {
                HashMap::new()
            },
            message_ids: HashMap::with_capacity(cap),
            ctx,
            unique_count: 0,
            duplicate_count: 0,
            tier1_hits: 0,
            tier2_hits: 0,
            stats: GroupingStats::default(),
            cross_mid_hash_seen: HashSet::new(),
            cross_mid_cluster: HashMap::new(),
        }
    }

    pub fn context(&self) -> &GroupingContext {
        &self.ctx
    }

    fn scope_prefix(&self, source_key: &str) -> String {
        match self.ctx.scope {
            DedupeScope::Global => String::new(),
            DedupeScope::PerSource => format!("{source_key}\0"),
        }
    }

    fn mid_key(&self, source_key: &str, mid: &str) -> String {
        format!("{}{mid}", self.scope_prefix(source_key))
    }

    fn hash_map_key(&self, source_key: &str, hash: &[u8; 32]) -> Vec<u8> {
        let mut k = self.scope_prefix(source_key).into_bytes();
        k.extend_from_slice(hash);
        k
    }

    fn tier2_bind_hash(item: &IndexItem, identity: IdentityLevel) -> [u8; 32] {
        if identity.is_strong() {
            item.strong_content_hash.unwrap_or(item.content_hash)
        } else {
            item.content_hash
        }
    }

    fn bound_by_for_tier2(identity: IdentityLevel) -> BoundBy {
        if identity.is_strong() {
            BoundBy::StrongContentHash
        } else {
            BoundBy::ContentHash
        }
    }

    fn tier_for_bound(b: BoundBy) -> DedupTier {
        match b {
            BoundBy::MessageId => DedupTier::MessageId,
            BoundBy::StrongContentHash => DedupTier::StrongContentHash,
            BoundBy::ContentHash | BoundBy::Seed => DedupTier::ContentHash,
        }
    }

    /// Classic API: assumes Tier-2 eligible, no strong hash, global scope.
    pub fn check_and_insert(
        &mut self,
        message_id: Option<&str>,
        content_hash: [u8; 32],
        msg_ref: MessageRef,
    ) -> DedupResult {
        self.check_and_insert_item(IndexItem::classic(message_id, content_hash, msg_ref))
    }

    /// Full 0076 insert path.
    pub fn check_and_insert_item(&mut self, item: IndexItem) -> DedupResult {
        let mid = mid_present(item.message_id.as_deref()).map(|s| s.to_string());
        let source = item.source_key.as_str();

        // ── Tier 1: Message-ID ─────────────────────────────────────────────
        if let Some(ref m) = mid {
            let key = self.mid_key(source, m);
            if let Some(slot) = self.message_ids.get(&key) {
                // Snapshot before mutating stats (§3.7: always report, optionally split).
                let slot_fps = (
                    slot.fp_body,
                    slot.fp_header,
                    slot.fp_recipients,
                    slot.fp_attachments,
                );
                let split = self.should_split_tier1(slot, &item);
                let original = slot.msg_ref.clone();
                self.note_tier1_divergence_fps(slot_fps, &item);
                if !split {
                    self.duplicate_count += 1;
                    self.tier1_hits += 1;
                    return DedupResult::DuplicateOf {
                        original,
                        tier: DedupTier::MessageId,
                        bound_by: BoundBy::MessageId,
                    };
                }
                // Fall through as if MID miss (split-only).
            }
        }

        // ── Tier 2: content / strong hash ──────────────────────────────────
        let eligible = item.tier2_eligible || !self.ctx.enforce_readable_body();
        if self.ctx.tier2_enabled && eligible {
            let bind_hash = Self::tier2_bind_hash(&item, self.ctx.identity);
            let hkey = self.hash_map_key(source, &bind_hash);
            if let Some(slot) = self.content_hashes.get(&hkey) {
                let (ok, new_bound) = mid_join_compatible(
                    slot.bound_mid.as_deref(),
                    mid.as_deref(),
                    self.ctx.block_cross_mid(),
                );
                if ok {
                    // Adopt MID on group when it had none.
                    let original = slot.msg_ref.clone();
                    if let Some(nb) = new_bound {
                        if let Some(s) = self.content_hashes.get_mut(&hkey) {
                            if s.bound_mid.is_none() {
                                s.bound_mid = Some(nb.clone());
                            }
                        }
                        // Register joining MID (D6 partial — split-only direction).
                        let mk = self.mid_key(source, &nb);
                        self.message_ids.entry(mk).or_insert_with(|| GroupSlot {
                            msg_ref: original.clone(),
                            bound_mid: Some(nb),
                            bind_hash,
                            fp_body: item.fp_body,
                            fp_header: item.fp_header,
                            fp_recipients: item.fp_recipients,
                            fp_attachments: item.fp_attachments,
                        });
                    }
                    self.duplicate_count += 1;
                    self.tier2_hits += 1;
                    let bound_by = Self::bound_by_for_tier2(self.ctx.identity);
                    return DedupResult::DuplicateOf {
                        original,
                        tier: Self::tier_for_bound(bound_by),
                        bound_by,
                    };
                } else {
                    // Cross-MID blocked
                    self.stats.cross_mid_blocked += 1;
                    if self.cross_mid_hash_seen.insert(hkey.clone()) {
                        self.stats.cross_mid_blocked_groups += 1;
                    }
                    if let Some(ref m) = mid {
                        let cluster = self.cross_mid_cluster.entry(hkey.clone()).or_default();
                        if let Some(existing) = self
                            .content_hashes
                            .get(&hkey)
                            .and_then(|s| s.bound_mid.clone())
                        {
                            cluster.insert(existing);
                        }
                        cluster.insert(m.clone());
                        self.stats.cross_mid_blocked_max_group = self
                            .stats
                            .cross_mid_blocked_max_group
                            .max(cluster.len() as u64);
                    }
                    // Fall through to create new unique (do not overwrite hash map).
                }
            }
        } else if self.ctx.tier2_enabled && !eligible {
            // Count block reason if we can distinguish — callers set tier2_eligible
            // after assessing; stats for unreadable/degenerate are tallied by caller
            // or via record_tier2_block.
        }

        // ── Unique — insert ────────────────────────────────────────────────
        let bind_hash = Self::tier2_bind_hash(&item, self.ctx.identity);
        let slot = GroupSlot {
            msg_ref: item.msg_ref.clone(),
            bound_mid: mid.clone(),
            bind_hash,
            fp_body: item.fp_body,
            fp_header: item.fp_header,
            fp_recipients: item.fp_recipients,
            fp_attachments: item.fp_attachments,
        };

        if let Some(ref m) = mid {
            let key = self.mid_key(source, m);
            self.message_ids.insert(key, slot_clone_ref(&slot));
        }
        if self.ctx.tier2_enabled && eligible {
            let hkey = self.hash_map_key(source, &bind_hash);
            // Only insert if vacant — never overwrite an earlier group (cross-MID case).
            self.content_hashes.entry(hkey).or_insert(slot);
        }

        self.unique_count += 1;
        DedupResult::Unique
    }

    /// Record a Tier-2 eligibility block in stats (call from scan when ineligible).
    pub fn record_tier2_block_unreadable(&mut self) {
        self.stats.tier2_blocked_unreadable_body += 1;
    }

    pub fn record_tier2_block_degenerate(&mut self) {
        self.stats.tier2_blocked_degenerate += 1;
    }

    pub fn record_tier2_block_crc_suspect(&mut self) {
        self.stats.tier2_blocked_crc_suspect += 1;
    }

    pub fn record_preview_over_budget(&mut self) {
        self.stats.tier2_preview_bytes_over_budget += 1;
    }

    /// Count an inline/embedded attachment omitted under `--identity-ignore-inline-attachments`.
    pub fn record_inline_attachment_ignored(&mut self) {
        self.stats.inline_attachments_ignored += 1;
    }

    /// Count an item whose recipient display string looks like X.500 (`/O=`).
    pub fn record_x500_recipient_item(&mut self) {
        self.stats.x500_recipient_items += 1;
    }

    /// Note: `--tier1-backfill` merge is a keep-set / `group_candidates` post-pass.
    /// Streaming insert cannot retroactively merge already-emitted uniques; CLI
    /// rejects the flag on `scan`/`dups`. Callers that need merges must re-group via
    /// [`crate::keepset::group_candidates_with_stats`] (keep-set / unique-*).
    pub fn note_tier1_backfill_streaming_limitation() -> &'static str {
        "tier1-backfill merge is keep-set/unique-* only; scan/dups reject the flag \
         (streaming DedupIndex cannot retro-merge)"
    }

    fn should_split_tier1(&self, slot: &GroupSlot, item: &IndexItem) -> bool {
        match self.ctx.tier1_verify {
            crate::grouping::Tier1Verify::Off => false,
            crate::grouping::Tier1Verify::Content => {
                let h = Self::tier2_bind_hash(item, self.ctx.identity);
                h != slot.bind_hash
            }
            crate::grouping::Tier1Verify::Body => item.fp_body != slot.fp_body,
        }
    }

    fn note_tier1_divergence_fps(&mut self, slot_fps: (u64, u64, u64, u64), item: &IndexItem) {
        let (gb, gh, gr, ga) = slot_fps;
        let body_diff = item.fp_body != gb;
        let meta_diff = item.fp_header != gh || item.fp_attachments != ga;
        let recip_diff = item.fp_recipients != gr;
        if body_diff {
            self.stats.tier1_divergent_body += 1;
        } else if recip_diff && !meta_diff {
            self.stats.tier1_divergent_recipients += 1;
        } else if meta_diff {
            self.stats.tier1_divergent_metadata += 1;
        }
    }

    /// Total messages processed.
    pub fn total(&self) -> u64 {
        self.unique_count + self.duplicate_count
    }

    /// Estimated memory savings in bytes (sum of duplicate message sizes).
    pub fn savings_bytes(&self) -> u64 {
        0
    }
}

fn slot_clone_ref(s: &GroupSlot) -> GroupSlot {
    GroupSlot {
        msg_ref: s.msg_ref.clone(),
        bound_mid: s.bound_mid.clone(),
        bind_hash: s.bind_hash,
        fp_body: s.fp_body,
        fp_header: s.fp_header,
        fp_recipients: s.fp_recipients,
        fp_attachments: s.fp_attachments,
    }
}

impl Default for DedupIndex {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_ref(subject: &str) -> MessageRef {
        MessageRef {
            pst_index: 0,
            pst_name: "test.pst".into(),
            folder_path: "Inbox".into(),
            nid: 0x1234,
            subject: subject.into(),
            submit_time: None,
            sender: "test@example.com".into(),
            size: 1024,
        }
    }

    #[test]
    fn test_unique_message() {
        let mut idx = DedupIndex::new();
        let result = idx.check_and_insert(Some("abc@example.com"), [0; 32], make_ref("Hello"));
        assert!(result.is_unique());
        assert_eq!(idx.unique_count, 1);
    }

    #[test]
    fn test_tier1_duplicate() {
        let mut idx = DedupIndex::new();
        idx.check_and_insert(Some("abc@example.com"), [0; 32], make_ref("Hello"));
        let result = idx.check_and_insert(Some("abc@example.com"), [1; 32], make_ref("Hello"));
        match result {
            DedupResult::DuplicateOf { tier, bound_by, .. } => {
                assert_eq!(tier, DedupTier::MessageId);
                assert_eq!(bound_by, BoundBy::MessageId);
            }
            _ => panic!("Expected duplicate"),
        }
        assert_eq!(idx.tier1_hits, 1);
    }

    #[test]
    fn test_tier2_duplicate() {
        let mut idx = DedupIndex::new();
        idx.check_and_insert(None, [42; 32], make_ref("No MID"));
        let result = idx.check_and_insert(None, [42; 32], make_ref("No MID"));
        match result {
            DedupResult::DuplicateOf { tier, bound_by, .. } => {
                assert_eq!(tier, DedupTier::ContentHash);
                assert_eq!(bound_by, BoundBy::ContentHash);
            }
            _ => panic!("Expected duplicate"),
        }
        assert_eq!(idx.tier2_hits, 1);
    }

    #[test]
    fn test_tier2_disabled_skips_content_hash() {
        let mut idx = DedupIndex::with_tier2(false);
        let r1 = idx.check_and_insert(None, [42; 32], make_ref("No MID"));
        assert!(r1.is_unique());
        let r2 = idx.check_and_insert(None, [42; 32], make_ref("No MID"));
        assert!(
            r2.is_unique(),
            "Tier 2 disabled: same content hash should NOT match"
        );
        assert_eq!(idx.tier2_hits, 0);
        assert_eq!(idx.unique_count, 2);
    }

    #[test]
    fn test_tier1_priority_over_tier2() {
        let mut idx = DedupIndex::new();
        idx.check_and_insert(Some("mid-a"), [1; 32], make_ref("First"));
        let result = idx.check_and_insert(Some("mid-a"), [2; 32], make_ref("Second"));
        match result {
            DedupResult::DuplicateOf { tier, .. } => {
                assert_eq!(tier, DedupTier::MessageId, "Tier 1 must win over Tier 2")
            }
            _ => panic!("Expected duplicate by Message-ID"),
        }
        assert_eq!(idx.tier1_hits, 1);
        assert_eq!(idx.tier2_hits, 0);
    }

    #[test]
    fn test_empty_message_id_treated_as_missing() {
        let mut idx = DedupIndex::new();
        let r1 = idx.check_and_insert(Some(""), [7; 32], make_ref("Empty MID"));
        assert!(r1.is_unique());
        let r2 = idx.check_and_insert(Some(""), [7; 32], make_ref("Empty MID 2"));
        match r2 {
            DedupResult::DuplicateOf { tier, .. } => {
                assert_eq!(tier, DedupTier::ContentHash)
            }
            _ => panic!("Expected Tier 2 duplicate for empty Message-ID"),
        }
    }

    #[test]
    fn test_tier2_disabled_empty_mid_is_unique() {
        let mut idx = DedupIndex::with_tier2(false);
        let r1 = idx.check_and_insert(Some(""), [7; 32], make_ref("Empty MID"));
        let r2 = idx.check_and_insert(Some(""), [7; 32], make_ref("Empty MID 2"));
        assert!(r1.is_unique());
        assert!(
            r2.is_unique(),
            "With Tier 2 disabled, empty Message-ID should not dedup"
        );
    }

    #[test]
    fn test_cross_tier_no_mid_joins_via_hash() {
        // Group bound m1 + item none → join allowed (table §3.4).
        let mut idx = DedupIndex::new();
        let r1 = idx.check_and_insert(Some("mid-1"), [5; 32], make_ref("Has MID"));
        assert!(r1.is_unique());
        let r2 = idx.check_and_insert(None, [5; 32], make_ref("No MID"));
        assert!(
            matches!(
                r2,
                DedupResult::DuplicateOf {
                    tier: DedupTier::ContentHash,
                    ..
                }
            ),
            "Same content hash should match even if first had Message-ID"
        );
    }

    #[test]
    fn cross_mid_blocked_by_default() {
        let mut idx = DedupIndex::new();
        idx.check_and_insert(Some("m1"), [9; 32], make_ref("A"));
        let r2 = idx.check_and_insert(Some("m2"), [9; 32], make_ref("B"));
        assert!(
            r2.is_unique(),
            "cross-MID must not merge under default guards"
        );
        assert_eq!(idx.stats.cross_mid_blocked, 1);
        assert_eq!(idx.unique_count, 2);
    }

    #[test]
    fn cross_mid_allowed_with_escape() {
        let ctx = GroupingContext {
            allow_cross_mid_tier2: true,
            ..Default::default()
        };
        let mut idx = DedupIndex::with_context(ctx);
        idx.check_and_insert(Some("m1"), [9; 32], make_ref("A"));
        let r2 = idx.check_and_insert(Some("m2"), [9; 32], make_ref("B"));
        assert!(matches!(r2, DedupResult::DuplicateOf { .. }));
    }

    #[test]
    fn ineligible_skips_tier2() {
        let mut idx = DedupIndex::new();
        let mut a = IndexItem::classic(None, [3; 32], make_ref("A"));
        a.tier2_eligible = false;
        let mut b = IndexItem::classic(None, [3; 32], make_ref("B"));
        b.tier2_eligible = false;
        assert!(idx.check_and_insert_item(a).is_unique());
        assert!(
            idx.check_and_insert_item(b).is_unique(),
            "ineligible items must not Tier-2 bind"
        );
    }

    #[test]
    fn mid_adopt_on_hash_join() {
        let mut idx = DedupIndex::new();
        // Seed with no MID
        idx.check_and_insert(None, [1; 32], make_ref("seed"));
        // Join with MID m1 → group adopts m1
        let r2 = idx.check_and_insert(Some("m1"), [1; 32], make_ref("join"));
        assert!(matches!(r2, DedupResult::DuplicateOf { .. }));
        // Third with same MID joins via Tier 1
        let r3 = idx.check_and_insert(Some("m1"), [99; 32], make_ref("later"));
        assert!(matches!(
            r3,
            DedupResult::DuplicateOf {
                bound_by: BoundBy::MessageId,
                ..
            }
        ));
    }
}
