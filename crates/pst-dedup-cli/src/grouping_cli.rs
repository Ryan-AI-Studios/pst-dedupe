//! Shared 0076 grouping-flag parsers and context builders for CLI surfaces.

use dedup_engine::{DedupeScope, GroupingContext, GroupingStats, IdentityLevel, Tier1Verify};

/// Parse `--strong-content-hash` value.
///
/// Live levels: `off|body|body-recip|body-recip-attach` (0086 enables attach content).
pub fn parse_identity_level(s: &str) -> Result<IdentityLevel, String> {
    IdentityLevel::parse(s).ok_or_else(|| {
        format!("invalid strong-content-hash '{s}': expected off|body|body-recip|body-recip-attach")
    })
}

/// Warning text when attach-content identity is combined with
/// `--identity-ignore-inline-attachments` (softens the byte-strict promise).
///
/// Returns `None` when no warning should be emitted. Pure helper for unit tests;
/// [`warn_ignore_inline_with_attach_content`] is the eprintln wrapper.
pub fn ignore_inline_with_attach_content_warning(
    identity: IdentityLevel,
    ignore_inline: bool,
) -> Option<&'static str> {
    if identity.includes_attach_content() && ignore_inline {
        Some(
            "warning: --identity-ignore-inline-attachments with body-recip-attach omits inline \
             attaches from both name:size and content digests (softens byte-strict identity; \
             logos/signatures still filtered)",
        )
    } else {
        None
    }
}

/// One-line stderr warning when attach-content identity is combined with
/// `--identity-ignore-inline-attachments` (softens the byte-strict promise).
pub fn warn_ignore_inline_with_attach_content(identity: IdentityLevel, ignore_inline: bool) {
    if let Some(msg) = ignore_inline_with_attach_content_warning(identity, ignore_inline) {
        eprintln!("{msg}");
    }
}

/// Reject `--no-attachments` with `body-recip-attach` (Choice B: cannot omit attach slots).
///
/// Returns `Err` when the combination would silently disable attach-content identity.
pub fn reject_no_attachments_with_attach_content(
    identity: IdentityLevel,
    no_attachments: bool,
) -> Result<(), String> {
    if identity.includes_attach_content() && no_attachments {
        return Err(
            "strong-content-hash body-recip-attach requires attachment enumeration; \
             remove --no-attachments (or use off|body|body-recip)"
                .into(),
        );
    }
    Ok(())
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
    allow_crc_suspect_tier2: bool,
    tier1_backfill: bool,
    ignore_inline_attachments: bool,
    no_attachments: bool,
) -> Result<GroupingContext, String> {
    let identity = parse_identity_level(strong_content_hash)?;
    reject_no_attachments_with_attach_content(identity, no_attachments)?;
    let scope = parse_dedupe_scope(dedupe_scope)?;
    let tier1_verify = parse_tier1_verify(tier1_verify)?;
    warn_ignore_inline_with_attach_content(identity, ignore_inline_attachments);
    Ok(GroupingContext {
        tier2_enabled: !no_tier2,
        scope,
        // Split-only guards on by default; escape hatches restore pre-0076.
        tier1_authority: !allow_cross_mid_tier2,
        require_readable_body: !allow_degenerate_tier2,
        identity,
        tier1_verify,
        allow_degenerate_tier2,
        allow_crc_suspect_tier2,
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
    if stats.tier2_blocked_crc_suspect > 0 {
        lines.push(format!(
            "  tier2 blocked (CRC_SUSPECT): {}",
            stats.tier2_blocked_crc_suspect
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
    if stats.strong_hash_attach_digested > 0 {
        lines.push(format!(
            "  strong-hash attach digested: {} ({} bytes)",
            stats.strong_hash_attach_digested, stats.strong_hash_attach_bytes
        ));
    }
    if stats.strong_hash_attach_truncated > 0 {
        lines.push(format!(
            "  strong-hash attach digest truncated (budget/cancel): {}",
            stats.strong_hash_attach_truncated
        ));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_body_recip_attach() {
        assert_eq!(
            parse_identity_level("body-recip-attach").unwrap(),
            IdentityLevel::BodyRecipAttach
        );
        assert_eq!(
            parse_identity_level("body_recip_attach").unwrap(),
            IdentityLevel::BodyRecipAttach
        );
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

    #[test]
    fn rejects_unknown_identity() {
        let err = parse_identity_level("bogus").unwrap_err();
        assert!(
            err.contains("off|body|body-recip|body-recip-attach"),
            "{err}"
        );
    }

    #[test]
    fn warn_helper_no_panic() {
        // eprintln wrapper: ensure call paths do not panic (stderr side effect).
        warn_ignore_inline_with_attach_content(IdentityLevel::BodyRecipAttach, true);
        warn_ignore_inline_with_attach_content(IdentityLevel::BodyRecipAttach, false);
        warn_ignore_inline_with_attach_content(IdentityLevel::Body, true);
    }

    #[test]
    fn rejects_no_attachments_with_body_recip_attach() {
        let err = reject_no_attachments_with_attach_content(IdentityLevel::BodyRecipAttach, true)
            .unwrap_err();
        assert!(err.contains("body-recip-attach"), "{err}");
        assert!(err.contains("--no-attachments"), "{err}");
        assert!(
            reject_no_attachments_with_attach_content(IdentityLevel::BodyRecipAttach, false)
                .is_ok()
        );
        assert!(reject_no_attachments_with_attach_content(IdentityLevel::Body, true).is_ok());
        assert!(reject_no_attachments_with_attach_content(IdentityLevel::Off, true).is_ok());
    }

    #[test]
    fn ignore_inline_attach_content_warning_text() {
        let msg = ignore_inline_with_attach_content_warning(IdentityLevel::BodyRecipAttach, true)
            .expect("warn when body-recip-attach + ignore-inline");
        assert!(
            msg.contains("--identity-ignore-inline-attachments"),
            "{msg}"
        );
        assert!(msg.contains("body-recip-attach"), "{msg}");
        assert!(msg.contains("softens byte-strict identity"), "{msg}");
        assert!(
            ignore_inline_with_attach_content_warning(IdentityLevel::BodyRecipAttach, false)
                .is_none()
        );
        assert!(ignore_inline_with_attach_content_warning(IdentityLevel::Body, true).is_none());
        assert!(ignore_inline_with_attach_content_warning(IdentityLevel::Off, true).is_none());
    }
}
