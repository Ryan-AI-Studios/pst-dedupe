//! Grouping context and bind provenance for identity binding (track 0076).
//!
//! Shared by [`crate::index::DedupIndex`] (streaming) and
//! [`crate::keepset::group_candidates`] (collect-all). One set of semantics.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Dedupe partition scope.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DedupeScope {
    /// Single global key maps (current / default deliverable).
    #[default]
    Global,
    /// Partition both maps by source path (`path_compare_key`).
    PerSource,
}

impl DedupeScope {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::PerSource => "per-source",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "global" => Some(Self::Global),
            "per-source" | "per_source" => Some(Self::PerSource),
            _ => None,
        }
    }
}

impl fmt::Display for DedupeScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Tier-2.5 / strong content identity level (layered, opt-in).
///
/// Ordered by store-to-store variance. Higher levels only **subdivide**
/// lower-level groups (v2 preimage = v1 preimage ∥ extras).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentityLevel {
    /// v1 content hash only (default).
    #[default]
    Off,
    /// Alias for Off (v1).
    V1,
    /// Full normalized body SHA-256 + char length.
    Body,
    /// Body + normalized display_to/cc/bcc.
    BodyRecip,
    /// Body + recipients + per-attachment content digests (may be deferred).
    BodyRecipAttach,
}

impl IdentityLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off | Self::V1 => "off",
            Self::Body => "body",
            Self::BodyRecip => "body-recip",
            Self::BodyRecipAttach => "body-recip-attach",
        }
    }

    /// `v1` or `v2` for decision CSV `identity_version`.
    pub fn identity_version(self) -> &'static str {
        match self {
            Self::Off | Self::V1 => "v1",
            Self::Body | Self::BodyRecip | Self::BodyRecipAttach => "v2",
        }
    }

    pub fn is_strong(self) -> bool {
        !matches!(self, Self::Off | Self::V1)
    }

    pub fn includes_body(self) -> bool {
        self.is_strong()
    }

    pub fn includes_recipients(self) -> bool {
        matches!(self, Self::BodyRecip | Self::BodyRecipAttach)
    }

    pub fn includes_attach_content(self) -> bool {
        matches!(self, Self::BodyRecipAttach)
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "off" | "v1" => Some(Self::Off),
            "body" => Some(Self::Body),
            "body-recip" | "body_recip" => Some(Self::BodyRecip),
            // Accepted in the enum for future wire-up / tests; CLI rejects this
            // value until D-0076-attach-content (see grouping_cli::parse_identity_level).
            "body-recip-attach" | "body_recip_attach" => Some(Self::BodyRecipAttach),
            _ => None,
        }
    }
}

impl fmt::Display for IdentityLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Opt-in subdivision of MID groups by content (§3.7).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tier1Verify {
    #[default]
    Off,
    /// Split MID group by full content / strong hash.
    Content,
    /// Split MID group by body component fingerprint only.
    Body,
}

impl Tier1Verify {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Content => "content",
            Self::Body => "body",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "off" => Some(Self::Off),
            "content" => Some(Self::Content),
            "body" => Some(Self::Body),
            _ => None,
        }
    }
}

impl fmt::Display for Tier1Verify {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// How an item was bound into its group (recorded at bind time — not guessed).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoundBy {
    /// First member of a new group (seed).
    Seed,
    /// Bound by Tier-1 Message-ID.
    MessageId,
    /// Bound by Tier-2 v1 content hash.
    ContentHash,
    /// Bound by Tier-2.5 strong content hash.
    StrongContentHash,
}

impl BoundBy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Seed => "seed",
            Self::MessageId => "message_id",
            Self::ContentHash => "content_hash",
            Self::StrongContentHash => "content_hash_strong",
        }
    }

    /// Decision CSV `tier` vocabulary for dup_of rows (closed set).
    pub fn tier_csv(self) -> Option<&'static str> {
        match self {
            Self::Seed => None,
            Self::MessageId => Some("message_id"),
            Self::ContentHash => Some("content_hash"),
            Self::StrongContentHash => Some("content_hash_strong"),
        }
    }
}

impl fmt::Display for BoundBy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Grouping policy for both streaming index and collect-all grouping.
///
/// **Default** enables the two split-only guards (`tier1_authority`,
/// `require_readable_body`). Pure pre-0076 semantics = those flags false
/// (`--allow-cross-mid-tier2` / `--allow-degenerate-tier2`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupingContext {
    /// Tier-2 content-hash fallback enabled (`--no-tier2` clears).
    pub tier2_enabled: bool,
    /// Global vs per-source partition.
    pub scope: DedupeScope,
    /// Block cross-MID Tier-2 merges (default true = split-only).
    pub tier1_authority: bool,
    /// Require readable non-degenerate body for Tier-2 bind (default true).
    pub require_readable_body: bool,
    /// Identity level for Tier-2 key (default Off = v1).
    pub identity: IdentityLevel,
    /// Opt-in MID-group subdivision.
    pub tier1_verify: Tier1Verify,
    /// Restore pre-0076 degenerate binding (`--allow-degenerate-tier2`).
    pub allow_degenerate_tier2: bool,
    /// Restore pre-0076 cross-MID Tier-2 merges (`--allow-cross-mid-tier2`).
    pub allow_cross_mid_tier2: bool,
    /// Opt-in merge of groups that share a MID discovered late (default off).
    pub tier1_backfill: bool,
    /// Opt-in: exclude inline/embedded attachments from attach component.
    pub ignore_inline_attachments: bool,
}

impl Default for GroupingContext {
    fn default() -> Self {
        Self {
            tier2_enabled: true,
            scope: DedupeScope::Global,
            // Split-only guards ON by default (0076).
            tier1_authority: true,
            require_readable_body: true,
            identity: IdentityLevel::Off,
            tier1_verify: Tier1Verify::Off,
            allow_degenerate_tier2: false,
            allow_cross_mid_tier2: false,
            tier1_backfill: false,
            ignore_inline_attachments: false,
        }
    }
}

impl GroupingContext {
    /// Pre-0076 grouping semantics (both split-only guards off).
    pub fn pre_0076() -> Self {
        Self {
            tier1_authority: false,
            require_readable_body: false,
            allow_degenerate_tier2: true,
            allow_cross_mid_tier2: true,
            ..Self::default()
        }
    }

    /// Convenience: Tier-2 on/off with otherwise-default 0076 guards.
    pub fn with_tier2(enabled: bool) -> Self {
        Self {
            tier2_enabled: enabled,
            ..Self::default()
        }
    }

    /// Effective cross-MID block (authority on and escape hatch off).
    pub fn block_cross_mid(&self) -> bool {
        self.tier1_authority && !self.allow_cross_mid_tier2
    }

    /// Effective unreadable/degenerate guard.
    pub fn enforce_readable_body(&self) -> bool {
        self.require_readable_body && !self.allow_degenerate_tier2
    }
}

/// Counters for identity-binding honesty (JSON + human summary).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupingStats {
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub tier2_blocked_unreadable_body: u64,
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub tier2_blocked_degenerate: u64,
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub cross_mid_blocked: u64,
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub cross_mid_blocked_groups: u64,
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub cross_mid_blocked_max_group: u64,
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub tier1_divergent_body: u64,
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub tier1_divergent_metadata: u64,
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub tier1_divergent_recipients: u64,
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub tier1_backfill_candidates: u64,
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub tier2_5_splits: u64,
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub tier2_5_splits_bcc_only: u64,
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub tier2_5_splits_recipients_only: u64,
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub x500_recipient_items: u64,
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub inline_attachments_ignored: u64,
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub strong_hash_attach_unread: u64,
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub tier2_preview_bytes_over_budget: u64,
}

fn is_zero_u64(v: &u64) -> bool {
    *v == 0
}

impl GroupingStats {
    /// Merge counters from another stats bag (additive).
    pub fn merge_from(&mut self, other: &GroupingStats) {
        self.tier2_blocked_unreadable_body += other.tier2_blocked_unreadable_body;
        self.tier2_blocked_degenerate += other.tier2_blocked_degenerate;
        self.cross_mid_blocked += other.cross_mid_blocked;
        self.cross_mid_blocked_groups += other.cross_mid_blocked_groups;
        self.cross_mid_blocked_max_group = self
            .cross_mid_blocked_max_group
            .max(other.cross_mid_blocked_max_group);
        self.tier1_divergent_body += other.tier1_divergent_body;
        self.tier1_divergent_metadata += other.tier1_divergent_metadata;
        self.tier1_divergent_recipients += other.tier1_divergent_recipients;
        self.tier1_backfill_candidates += other.tier1_backfill_candidates;
        self.tier2_5_splits += other.tier2_5_splits;
        self.tier2_5_splits_bcc_only += other.tier2_5_splits_bcc_only;
        self.tier2_5_splits_recipients_only += other.tier2_5_splits_recipients_only;
        self.x500_recipient_items += other.x500_recipient_items;
        self.inline_attachments_ignored += other.inline_attachments_ignored;
        self.strong_hash_attach_unread += other.strong_hash_attach_unread;
        self.tier2_preview_bytes_over_budget += other.tier2_preview_bytes_over_budget;
    }

    /// True when any guard fired (for human-summary branch).
    pub fn any_guard_fired(&self) -> bool {
        self.tier2_blocked_unreadable_body > 0
            || self.tier2_blocked_degenerate > 0
            || self.cross_mid_blocked > 0
            || self.tier2_preview_bytes_over_budget > 0
            || self.tier1_divergent_body > 0
    }
}

/// Normalize a display recipient string for Tier-2.5: trim, lowercase, split on
/// `;`, sort tokens, rejoin. Display names — not SMTP addresses.
pub fn normalize_recipients(s: &str) -> String {
    let mut parts: Vec<String> = s
        .split(';')
        .map(|p| p.trim().to_lowercase())
        .filter(|p| !p.is_empty())
        .collect();
    parts.sort();
    parts.join(";")
}

/// Cheap detector: recipient string contains an `/O=`-prefixed X.500 segment.
pub fn recipient_has_x500(s: &str) -> bool {
    s.to_ascii_uppercase().contains("/O=")
}

/// MID compatibility for Tier-2 join (§3.4 table).
///
/// Empty string counts as absent. Returns whether the item may join the group,
/// and the group's new bound MID after a successful join (adopt item MID when
/// group has none).
pub fn mid_join_compatible(
    group_bound_mid: Option<&str>,
    item_mid: Option<&str>,
    block_cross_mid: bool,
) -> (bool, Option<String>) {
    let g = group_bound_mid.filter(|m| !m.is_empty());
    let i = item_mid.filter(|m| !m.is_empty());
    match (g, i) {
        (None, None) => (true, None),
        (None, Some(m)) => (true, Some(m.to_string())),
        (Some(gm), None) => (true, Some(gm.to_string())),
        (Some(gm), Some(im)) if gm == im => (true, Some(gm.to_string())),
        (Some(gm), Some(_)) if !block_cross_mid => (true, Some(gm.to_string())),
        (Some(_), Some(_)) => (false, None),
    }
}

/// Normalize optional MID: empty → None.
pub fn mid_present(mid: Option<&str>) -> Option<&str> {
    mid.filter(|m| !m.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mid_table_cross_blocked() {
        let (ok, _) = mid_join_compatible(Some("m1"), Some("m2"), true);
        assert!(!ok);
        let (ok, bound) = mid_join_compatible(Some("m1"), Some("m2"), false);
        assert!(ok);
        assert_eq!(bound.as_deref(), Some("m1"));
    }

    #[test]
    fn mid_table_adopt() {
        let (ok, bound) = mid_join_compatible(None, Some("m1"), true);
        assert!(ok);
        assert_eq!(bound.as_deref(), Some("m1"));
    }

    #[test]
    fn recipient_normalize_sort() {
        assert_eq!(
            normalize_recipients("A@x.com; b@X.com"),
            normalize_recipients("b@x.com;a@x.com")
        );
    }

    #[test]
    fn x500_detect() {
        assert!(recipient_has_x500("/O=EXCHANGELABS/OU=…"));
        assert!(!recipient_has_x500("Smith, John"));
    }

    #[test]
    fn default_guards_on() {
        let ctx = GroupingContext::default();
        assert!(ctx.tier1_authority);
        assert!(ctx.require_readable_body);
        assert!(ctx.block_cross_mid());
        assert!(ctx.enforce_readable_body());
    }

    #[test]
    fn pre_0076_guards_off() {
        let ctx = GroupingContext::pre_0076();
        assert!(!ctx.block_cross_mid());
        assert!(!ctx.enforce_readable_body());
    }
}
