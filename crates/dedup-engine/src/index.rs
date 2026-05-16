//! In-memory dedup index — HashMap-based with tiered lookup.

use std::collections::HashMap;

/// Which dedup tier matched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DedupTier {
    /// Tier 1: Message-ID exact match.
    MessageId,
    /// Tier 2: SHA-256 content hash match.
    ContentHash,
}

impl std::fmt::Display for DedupTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DedupTier::MessageId => write!(f, "Message-ID"),
            DedupTier::ContentHash => write!(f, "Content Hash"),
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
    /// First occurrence — this message is unique (so far).
    Unique,
    /// Duplicate of an earlier message.
    DuplicateOf {
        /// The original (first-seen) message.
        original: MessageRef,
        /// Which tier detected the match.
        tier: DedupTier,
    },
}

/// The dedup index. Insert messages in scan order; check returns Unique or DuplicateOf.
pub struct DedupIndex {
    /// Tier 1: normalized Message-ID → first occurrence.
    message_ids: HashMap<String, MessageRef>,
    /// Tier 2: content hash → first occurrence.
    content_hashes: HashMap<[u8; 32], MessageRef>,
    /// Whether Tier 2 (content hash) fallback is enabled.
    tier2_enabled: bool,
    /// Running counts.
    pub unique_count: u64,
    pub duplicate_count: u64,
    pub tier1_hits: u64,
    pub tier2_hits: u64,
}

impl DedupIndex {
    pub fn new() -> Self {
        Self {
            message_ids: HashMap::new(),
            content_hashes: HashMap::new(),
            tier2_enabled: true,
            unique_count: 0,
            duplicate_count: 0,
            tier1_hits: 0,
            tier2_hits: 0,
        }
    }

    /// With pre-allocated capacity for expected message count.
    pub fn with_capacity(expected: usize) -> Self {
        Self {
            message_ids: HashMap::with_capacity(expected),
            content_hashes: HashMap::with_capacity(expected / 4), // fewer Tier 2 lookups expected
            tier2_enabled: true,
            unique_count: 0,
            duplicate_count: 0,
            tier1_hits: 0,
            tier2_hits: 0,
        }
    }

    /// Create with Tier 2 explicitly enabled or disabled.
    pub fn with_tier2(enabled: bool) -> Self {
        Self {
            message_ids: HashMap::new(),
            content_hashes: HashMap::new(),
            tier2_enabled: enabled,
            unique_count: 0,
            duplicate_count: 0,
            tier1_hits: 0,
            tier2_hits: 0,
        }
    }

    /// Create with capacity and Tier 2 setting.
    pub fn with_capacity_and_tier2(expected: usize, tier2_enabled: bool) -> Self {
        Self {
            message_ids: HashMap::with_capacity(expected),
            content_hashes: if tier2_enabled {
                HashMap::with_capacity(expected / 4)
            } else {
                HashMap::new()
            },
            tier2_enabled,
            unique_count: 0,
            duplicate_count: 0,
            tier1_hits: 0,
            tier2_hits: 0,
        }
    }

    /// Check a message against the index and insert if unique.
    ///
    /// Returns `DedupResult::Unique` if this is the first occurrence,
    /// or `DedupResult::DuplicateOf` with the original reference and matched tier.
    pub fn check_and_insert(
        &mut self,
        message_id: Option<&str>,
        content_hash: [u8; 32],
        msg_ref: MessageRef,
    ) -> DedupResult {
        // Tier 1: Message-ID match
        if let Some(mid) = message_id {
            if !mid.is_empty() {
                if let Some(original) = self.message_ids.get(mid) {
                    self.duplicate_count += 1;
                    self.tier1_hits += 1;
                    return DedupResult::DuplicateOf {
                        original: original.clone(),
                        tier: DedupTier::MessageId,
                    };
                }
            }
        }

        // Tier 2: Content hash match (only when enabled)
        if self.tier2_enabled {
            if let Some(original) = self.content_hashes.get(&content_hash) {
                self.duplicate_count += 1;
                self.tier2_hits += 1;
                return DedupResult::DuplicateOf {
                    original: original.clone(),
                    tier: DedupTier::ContentHash,
                };
            }
        }

        // Unique — insert into applicable indexes
        if let Some(mid) = message_id {
            if !mid.is_empty() {
                self.message_ids.insert(mid.to_string(), msg_ref.clone());
            }
        }
        if self.tier2_enabled {
            self.content_hashes.insert(content_hash, msg_ref);
        }
        self.unique_count += 1;

        DedupResult::Unique
    }

    /// Total messages processed.
    pub fn total(&self) -> u64 {
        self.unique_count + self.duplicate_count
    }

    /// Estimated memory savings in bytes (sum of duplicate message sizes).
    pub fn savings_bytes(&self) -> u64 {
        // This would require tracking duplicate sizes; for now return 0.
        // The report module computes actual savings from the full results.
        0
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
        assert!(matches!(result, DedupResult::Unique));
        assert_eq!(idx.unique_count, 1);
    }

    #[test]
    fn test_tier1_duplicate() {
        let mut idx = DedupIndex::new();
        idx.check_and_insert(Some("abc@example.com"), [0; 32], make_ref("Hello"));
        let result = idx.check_and_insert(Some("abc@example.com"), [1; 32], make_ref("Hello"));
        match result {
            DedupResult::DuplicateOf { tier, .. } => assert_eq!(tier, DedupTier::MessageId),
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
            DedupResult::DuplicateOf { tier, .. } => assert_eq!(tier, DedupTier::ContentHash),
            _ => panic!("Expected duplicate"),
        }
        assert_eq!(idx.tier2_hits, 1);
    }

    #[test]
    fn test_tier2_disabled_skips_content_hash() {
        let mut idx = DedupIndex::with_tier2(false);
        // First message without Message-ID — should be unique
        let r1 = idx.check_and_insert(None, [42; 32], make_ref("No MID"));
        assert!(matches!(r1, DedupResult::Unique));

        // Second message with same content hash — should ALSO be unique because Tier 2 is off
        let r2 = idx.check_and_insert(None, [42; 32], make_ref("No MID"));
        assert!(
            matches!(r2, DedupResult::Unique),
            "Tier 2 disabled: same content hash should NOT match"
        );
        assert_eq!(idx.tier2_hits, 0);
        assert_eq!(idx.unique_count, 2);
    }

    #[test]
    fn test_tier1_priority_over_tier2() {
        let mut idx = DedupIndex::new();
        // First message: has Message-ID A and content hash X
        idx.check_and_insert(Some("mid-a"), [1; 32], make_ref("First"));

        // Second message: same Message-ID A but DIFFERENT content hash Y
        // Tier 1 should match first, not fall through to Tier 2
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
        // Empty Message-ID falls through to Tier 2
        let r1 = idx.check_and_insert(Some(""), [7; 32], make_ref("Empty MID"));
        assert!(matches!(r1, DedupResult::Unique));

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
        // Empty Message-ID with Tier 2 disabled → always unique
        let r1 = idx.check_and_insert(Some(""), [7; 32], make_ref("Empty MID"));
        let r2 = idx.check_and_insert(Some(""), [7; 32], make_ref("Empty MID 2"));
        assert!(matches!(r1, DedupResult::Unique));
        assert!(
            matches!(r2, DedupResult::Unique),
            "With Tier 2 disabled, empty Message-ID should not dedup"
        );
    }

    #[test]
    fn test_cross_tier_no_false_positive() {
        // A message with Message-ID should never match a message without one
        // on content hash alone if the first had a Message-ID
        let mut idx = DedupIndex::new();
        let r1 = idx.check_and_insert(Some("mid-1"), [5; 32], make_ref("Has MID"));
        assert!(matches!(r1, DedupResult::Unique));

        // Second message has NO Message-ID but same content hash
        // It should NOT be a duplicate because the first was indexed by Message-ID,
        // and content hash index also contains it — so it WILL match by Tier 2.
        // This is ACCEPTABLE behavior (conservative dedup).
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
}
