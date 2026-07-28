//! Shared 0076 grouping-flag parsers and context builders for CLI surfaces.

use dedup_engine::{DedupeScope, GroupingContext, GroupingStats, IdentityLevel, Tier1Verify};

/// Parse `--strong-content-hash` value.
///
/// Live levels: `off|body|body-recip`. `body-recip-attach` is rejected until
/// **D-0076-attach-content** wires 0074 attach digests (do not silently accept).
pub fn parse_identity_level(s: &str) -> Result<IdentityLevel, String> {
    match s {
        "body-recip-attach" | "body_recip_attach" => Err(
            "strong-content-hash 'body-recip-attach' is not enabled yet \
             (deferred D-0076-attach-content: attachment content digests via 0074 probe). \
             Use off|body|body-recip"
                .into(),
        ),
        other => IdentityLevel::parse(other).ok_or_else(|| {
            format!("invalid strong-content-hash '{other}': expected off|body|body-recip")
        }),
    }
}

pub fn parse_dedupe_scope(s: &str) -> Result<DedupeScope, String> {
    DedupeScope::parse(s)
        .ok_or_else(|| format!("invalid dedupe-scope '{s}': expected global|per-source"))
}

pub fn parse_tier1_verify(s: &str) -> Result<Tier1Verify, String> {
    Tier1Verify::parse(s)
        .ok_or_else(|| format!("invalid tier1-verify '{s}': expected off|content|body"))
}

/// Build a [`GroupingContext`] from CLI flags common to scan/dups/keep-set/unique-*.
#[allow(clippy::too_many_arguments)]
pub fn grouping_context_from_cli(
    no_tier2: bool,
    strong_content_hash: &str,
    dedupe_scope: &str,
    tier1_verify: &str,
    allow_cross_mid_tier2: bool,
    allow_degenerate_tier2: bool,
    tier1_backfill: bool,
    ignore_inline_attachments: bool,
) -> Result<GroupingContext, String> {
    let identity = parse_identity_level(strong_content_hash)?;
    let scope = parse_dedupe_scope(dedupe_scope)?;
    let tier1_verify = parse_tier1_verify(tier1_verify)?;
    Ok(GroupingContext {
        tier2_enabled: !no_tier2,
        scope,
        // Split-only guards on by default; escape hatches restore pre-0076.
        tier1_authority: !allow_cross_mid_tier2,
        require_readable_body: !allow_degenerate_tier2,
        identity,
        tier1_verify,
        allow_degenerate_tier2,
        allow_cross_mid_tier2,
        tier1_backfill,
        ignore_inline_attachments,
    })
}

/// Format grouping stats for human stderr summaries (scan / keep-set / unique-pst).
pub fn format_grouping_stats_human(stats: &GroupingStats) -> Vec<String> {
    let mut lines = Vec::new();
    if stats.tier2_blocked_unreadable_body > 0 {
        lines.push(format!(
            "  tier2 blocked (unreadable body): {}",
            stats.tier2_blocked_unreadable_body
        ));
    }
    if stats.tier2_blocked_degenerate > 0 {
        lines.push(format!(
            "  tier2 blocked (degenerate preimage): {}",
            stats.tier2_blocked_degenerate
        ));
    }
    if stats.cross_mid_blocked > 0 {
        lines.push(format!(
            "  cross-MID Tier-2 blocked: {} items ({} groups, max cluster {})",
            stats.cross_mid_blocked,
            stats.cross_mid_blocked_groups,
            stats.cross_mid_blocked_max_group
        ));
    }
    if stats.tier2_preview_bytes_over_budget > 0 {
        lines.push(format!(
            "  content_hash preview bytes over budget: {} (non-Latin rehash population)",
            stats.tier2_preview_bytes_over_budget
        ));
    }
    if stats.tier1_divergent_body > 0 {
        lines.push(format!(
            "  note: {} MID group(s) have divergent body text (Purview edited-unsent class)",
            stats.tier1_divergent_body
        ));
    }
    if stats.tier1_divergent_metadata > 0 {
        lines.push(format!(
            "  MID groups divergent metadata only: {}",
            stats.tier1_divergent_metadata
        ));
    }
    if stats.tier1_divergent_recipients > 0 {
        lines.push(format!(
            "  MID groups divergent recipients only: {}",
            stats.tier1_divergent_recipients
        ));
    }
    if stats.tier1_backfill_candidates > 0 {
        lines.push(format!(
            "  tier1 backfill candidates: {} (late-compatible MID under same content hash; \
             merged when --tier1-backfill is set on keep-set/unique-*; \
             scan/dups reject the flag — streaming index cannot retro-merge)",
            stats.tier1_backfill_candidates
        ));
    }
    if stats.tier2_5_splits > 0 {
        lines.push(format!("  tier2.5 splits: {}", stats.tier2_5_splits));
    }
    if stats.tier2_5_splits_recipients_only > 0 {
        lines.push(format!(
            "  tier2.5 splits (recipients only): {}",
            stats.tier2_5_splits_recipients_only
        ));
    }
    if stats.tier2_5_splits_bcc_only > 0 {
        lines.push(format!(
            "  tier2.5 splits (BCC only): {}",
            stats.tier2_5_splits_bcc_only
        ));
    }
    if stats.x500_recipient_items > 0 {
        lines.push(format!(
            "  X.500-looking recipient display strings: {}",
            stats.x500_recipient_items
        ));
    }
    if stats.inline_attachments_ignored > 0 {
        lines.push(format!(
            "  inline attachments ignored in identity: {}",
            stats.inline_attachments_ignored
        ));
    }
    if stats.strong_hash_attach_unread > 0 {
        lines.push(format!(
            "  strong-hash attach unread fallbacks: {}",
            stats.strong_hash_attach_unread
        ));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_body_recip_attach() {
        let err = parse_identity_level("body-recip-attach").unwrap_err();
        assert!(err.contains("D-0076-attach-content"), "{err}");
        assert!(err.contains("off|body|body-recip"), "{err}");
    }

    #[test]
    fn accepts_live_levels() {
        assert_eq!(parse_identity_level("off").unwrap(), IdentityLevel::Off);
        assert_eq!(parse_identity_level("body").unwrap(), IdentityLevel::Body);
        assert_eq!(
            parse_identity_level("body-recip").unwrap(),
            IdentityLevel::BodyRecip
        );
    }
}
